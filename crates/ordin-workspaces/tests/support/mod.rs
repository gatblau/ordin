//! The fixture of this crate's tests: a real SQLite journal, Rules Engine, Agent Folder Manager,
//! File Service, Approval Service and Identity Service in a temporary folder, the real SharePoint,
//! Confluence and Jira adapters pointed at one `wiremock` stand-in on loopback, and the Workspace
//! Service over all of them. The stand-in's replies follow the adapters' own specs (the Graph,
//! Confluence Cloud and Jira Cloud surfaces they list); no real network is used. Shared by the unit
//! tests (`ordin_workspaces` is the crate itself there) and the integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_confluence::{ConfluenceAdapter, ConfluenceDeps, ConfluenceSettings};
use ordin_files::{FileService, FileServiceDeps};
use ordin_identity::{
    IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig, SecretName,
    SecretString,
};
use ordin_jira::{JiraAdapter, JiraDeps, JiraEndpoints};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalEntry, JournalEvent,
    JournalOptions, MigrationRegistry, ModuleMigrations, SystemClock, MIGRATIONS,
};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, ProgramKind, Proposal, RulesConfig,
    RulesEngine, RulesHost, RunKey,
};
use ordin_sharepoint::{SharePointAdapter, SharePointDeps, SharePointLocation};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{BackendId, RunId};
use ordin_workspaces::{WorkspaceDeps, WorkspaceService};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::watch;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

pub const MS_TOKEN: &str = "at-ms-secret-token-0001";
pub const ATLASSIAN_TOKEN: &str = "at-atlassian-secret-token-0001";
pub const JIRA_TOKEN: &str = "ATATT3xSecretJiraTokenValue0123456789";
pub const JIRA_SITE: &str = "acme.atlassian.net";
pub const JIRA_EMAIL: &str = "priya.shah@acme.co.uk";
pub const CONFLUENCE_SITE: &str = "https://acme.atlassian.net";
pub const CLOUD: &str = "cloud-1";
pub const SITE: &str = "acme.sharepoint.com,1f0c,9a2e";
pub const DRIVE: &str = "b!x9Fq";
pub const FOLDER: &str = "01ABCDEF2GHIJ";
pub const FILE: &str = "01ABCDEF7KLMN";
pub const SPACE_ID: &str = "65010";
pub const PARENT: &str = "4000";
const MS_SCOPES: [&str; 2] = [
    "https://graph.microsoft.com/Files.ReadWrite.All",
    "https://graph.microsoft.com/Sites.Read.All",
];
const CONFLUENCE_SCOPES: [&str; 12] = [
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
        let dir = std::env::temp_dir().join(format!("ordin-workspaces-{}", uuid::Uuid::now_v7()));
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

/// What a test varies about the fixture.
#[derive(Clone)]
pub struct Options {
    /// The personal server has no Confluence adapter (2B-3: Confluence is D only).
    pub confluence: bool,
    /// The personal server has no Jira adapter.
    pub jira: bool,
    pub on_server: bool,
    /// A Microsoft credential for SharePoint.
    pub microsoft: bool,
    /// An Atlassian credential for Confluence.
    pub atlassian: bool,
    /// A Jira API token.
    pub jira_token: bool,
    /// The Rules Engine's application data root is the data root itself, as the program sets it
    /// (FileService.md 2.2 exempts the `working_copy` kind from the protected-location check).
    pub rules_cover_data_root: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            confluence: true,
            jira: true,
            on_server: false,
            microsoft: true,
            atlassian: true,
            jira_token: true,
            rules_cover_data_root: false,
        }
    }
}

pub struct Env {
    pub dir: TempDir,
    pub data_root: PathBuf,
    pub documents: PathBuf,
    pub protected: PathBuf,
    pub mock: MockServer,
    pub journal: Arc<Journal>,
    pub pool: SqlitePool,
    pub files: Arc<FileService>,
    pub rules: Arc<RulesEngine>,
    pub approvals: Arc<ApprovalService>,
    pub identity: Arc<IdentityService>,
    pub sharepoint: Arc<SharePointAdapter>,
    pub confluence: Option<Arc<ConfluenceAdapter>>,
    pub jira: Option<Arc<JiraAdapter>>,
    pub service: Arc<WorkspaceService>,
    pub settings: watch::Sender<ConfluenceSettings>,
}

fn identity_config(dir: &Path, key_file: &Path) -> IdentityConfig {
    IdentityConfig {
        program: Program::Desktop,
        data_root: dir.to_path_buf(),
        keychain_service: "io.gatblau.ordin.workspaces-test".to_owned(),
        secret_backend: SecretBackendConfig::EncryptedFile {
            key_source: KeySource::KeyFile(key_file.to_path_buf()),
        },
        github_app_client_id: None,
        http: reqwest::Client::new(),
        browser: None,
    }
}

/// Seeds a credential the way a completed sign-in leaves it: the token blob in the secret store, the
/// two rows in the journal database.
async fn seed_credential(
    identity: &IdentityService,
    pool: &SqlitePool,
    kind: &str,
    account: &str,
    host: &str,
    token: &str,
    scopes: &[&str],
) {
    let id = ordin_types::CredentialId::new();
    let name = format!("credential/{kind}/{id}");
    let blob = json!({
        "access_token": token,
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
    let now = Timestamp::now().to_wire();
    sqlx::query(
        "INSERT INTO credentials (id, kind, account_label, keychain_ref, expires_at, state, created_at) \
         VALUES (?1, ?2, ?3, ?4, NULL, 'active', ?5)",
    )
    .bind(id.to_string())
    .bind(kind)
    .bind(account)
    .bind(&name)
    .bind(&now)
    .execute(pool)
    .await
    .expect("credential row");
    sqlx::query(
        "INSERT INTO credential_meta (credential_id, provider_host, scopes, needs_sign_in) VALUES (?1, ?2, ?3, 0)",
    )
    .bind(id.to_string())
    .bind(host)
    .bind(serde_json::to_string(scopes).expect("scopes"))
    .execute(pool)
    .await
    .expect("credential meta");
}

impl Env {
    pub async fn new() -> Env {
        Env::with(Options::default()).await
    }

    pub async fn with(options: Options) -> Env {
        let dir = TempDir::new();
        let data_root = dir.path().join("data");
        let documents = dir.path().join("Documents");
        let protected = dir.path().join("protected");
        let rules_data = if options.rules_cover_data_root {
            data_root.clone()
        } else {
            dir.path().join("rules-data")
        };
        for folder in [&data_root, &documents, &protected, &rules_data] {
            std::fs::create_dir_all(folder).expect("folder");
        }
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: data_root.join("journal.db"),
            backup_dir: data_root.join("backups"),
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
            .register(ConfluenceAdapter::migrations())
            .expect("confluence");
        registry
            .register(ordin_workspaces::migrations())
            .expect("workspaces");
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
            profile,
            approvals.clone(),
            Arc::new(Host),
            RulesConfig {
                ordin_servers: vec!["ordin".to_owned()],
                ordin_server_prefixes: vec![],
                protected_roots: vec![protected.clone(), PathBuf::from("/System")],
                app_data_root: rules_data,
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

        let mock = MockServer::start().await;
        let key_file = dir.path().join("secrets.key");
        std::fs::write(&key_file, STANDARD.encode([7u8; 32])).expect("key file");
        let seeding =
            IdentityService::new(identity_config(dir.path(), &key_file), Some(pool.clone()))
                .await
                .expect("identity");
        if options.microsoft {
            seed_credential(
                &seeding,
                &pool,
                "microsoft",
                "sam@acme.example",
                "login.microsoftonline.com",
                MS_TOKEN,
                &MS_SCOPES,
            )
            .await;
        }
        if options.atlassian {
            seed_credential(
                &seeding,
                &pool,
                "confluence",
                "acme.atlassian.net",
                "acme.atlassian.net",
                ATLASSIAN_TOKEN,
                &CONFLUENCE_SCOPES,
            )
            .await;
        }
        drop(seeding);
        let identity = Arc::new(
            IdentityService::new(identity_config(dir.path(), &key_file), Some(pool.clone()))
                .await
                .expect("identity"),
        );
        if options.jira_token {
            identity
                .save_jira_token(
                    JIRA_SITE,
                    JIRA_EMAIL,
                    SecretString::new(JIRA_TOKEN.to_owned()),
                    "712020:7d2a-priya",
                    "Priya Shah",
                )
                .await
                .expect("the Jira token is kept");
        }
        let sharepoint = Arc::new(
            SharePointAdapter::new(SharePointDeps {
                identity: identity.clone(),
                journal: journal.as_ref().clone(),
                http: reqwest::Client::new(),
            })
            .expect("sharepoint adapter")
            .with_stand_in(
                &format!("{}/v1.0", mock.uri()),
                0.001,
                Duration::from_secs(5),
            )
            .expect("stand-in"),
        );
        let (settings, receiver) = watch::channel(ConfluenceSettings {
            site: Some(url::Url::parse(CONFLUENCE_SITE).expect("site")),
        });
        let confluence = options.confluence.then(|| {
            ConfluenceAdapter::with_stand_in(
                ConfluenceDeps {
                    journal: journal.clone(),
                    identity: identity.clone(),
                    approvals: approvals.clone(),
                    files: files.clone(),
                    http: reqwest::Client::new(),
                    settings: receiver,
                    word_to_markdown: None,
                },
                &mock.uri(),
                0.001,
                Duration::from_secs(5),
            )
        });
        let jira = options.jira.then(|| {
            JiraAdapter::with_endpoints(
                JiraDeps {
                    journal: journal.clone(),
                    identity: identity.clone(),
                    http: reqwest::Client::new(),
                },
                JiraEndpoints {
                    site_base_url: Some(mock.uri()),
                    gateway_base_url: Some(mock.uri()),
                    request_timeout: Some(Duration::from_secs(5)),
                },
            )
        });
        let service = WorkspaceService::new(WorkspaceDeps {
            journal: journal.as_ref().clone(),
            rules: rules.clone(),
            files: files.clone(),
            sharepoint: sharepoint.clone(),
            confluence: confluence.clone(),
            jira: jira.clone(),
            data_root: data_root.clone(),
            documents_dir: documents.clone(),
            on_server: options.on_server,
        })
        .await
        .expect("workspace service");
        let env = Env {
            dir,
            data_root,
            documents,
            protected,
            mock,
            journal,
            pool,
            files,
            rules,
            approvals,
            identity,
            sharepoint,
            confluence,
            jira,
            service,
            settings,
        };
        env.mount_resources().await;
        env
    }

    /// `GET …/oauth/token/accessible-resources`: the gateway's look-up of the Confluence site.
    async fn mount_resources(&self) {
        Mock::given(method("GET"))
            .and(path("/oauth/token/accessible-resources"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"id": CLOUD, "url": CONFLUENCE_SITE, "name": "acme", "scopes": []}
            ])))
            .mount(&self.mock)
            .await;
    }

    /// A folder in the temporary directory, made now.
    pub fn folder(&self, name: &str) -> PathBuf {
        let folder = self.dir.path().join(name);
        std::fs::create_dir_all(&folder).expect("folder");
        folder
    }

    /// A folder with a `.git` directory whose `HEAD` names `branch`, with the given `config` text and,
    /// when `origin_commit` is present, `refs/remotes/origin/<branch>`.
    pub fn git_folder(
        &self,
        name: &str,
        branch: &str,
        config: &str,
        origin_commit: Option<&str>,
    ) -> PathBuf {
        let folder = self.folder(name);
        let git = folder.join(".git");
        std::fs::create_dir_all(git.join("refs").join("remotes").join("origin"))
            .expect("git folder");
        std::fs::write(git.join("HEAD"), format!("ref: refs/heads/{branch}\n")).expect("HEAD");
        std::fs::write(git.join("config"), config).expect("config");
        if let Some(commit) = origin_commit {
            let reference = git.join("refs").join("remotes").join("origin").join(branch);
            std::fs::create_dir_all(reference.parent().expect("parent")).expect("refs");
            std::fs::write(reference, format!("{commit}\n")).expect("ref");
        }
        folder
    }

    pub async fn create_workspace(&self, name: &str) -> ordin_workspaces::Workspace {
        self.service
            .create(name.to_owned())
            .await
            .expect("create the workspace")
    }

    /// Every journal event of a kind, oldest first.
    pub async fn events(&self, kind: &str) -> Vec<JournalEvent> {
        self.journal
            .events_since(0, 10_000)
            .await
            .expect("events")
            .into_iter()
            .filter(|e| e.kind == kind)
            .collect()
    }

    /// Every journal entry of a kind.
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
            .expect("entries")
            .items
    }

    /// A count from a test's own literal statement.
    pub async fn scalar(&self, sql: &'static str) -> i64 {
        use sqlx::Row as _;
        sqlx::query(sql)
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .get::<i64, _>(0)
    }

    /// Every request the stand-in received, as (method, path).
    pub async fn requests(&self) -> Vec<(String, String)> {
        self.mock
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(|r| (r.method.to_string(), r.url.path().to_owned()))
            .collect()
    }

    /// The requests that reached Jira's paths.
    pub async fn jira_requests(&self) -> Vec<(String, String)> {
        self.requests()
            .await
            .into_iter()
            .filter(|(_, p)| p.starts_with("/rest/api/"))
            .collect()
    }

    /// A Confluence link of the workspace's own folder in state `proposed`, with its approval asked.
    pub async fn proposed_confluence_link(
        &self,
        ws: &ordin_workspaces::Workspace,
    ) -> (
        ordin_workspaces::ConfluenceLinkId,
        ordin_workspaces::ApprovalId,
    ) {
        mount_where(self).await;
        let link = self
            .service
            .propose_confluence_link(
                ws.id,
                ws.own_folder_place_id,
                String::new(),
                parent_location(),
            )
            .await
            .expect("propose the Confluence link");
        (
            link.id,
            link.approval_id.expect("the approval id is stored"),
        )
    }

    /// Delivers `approval_decided` to the service as the Journal would.
    pub async fn decide(&self, approval: ordin_workspaces::ApprovalId, state: &str) {
        use ordin_journal::EventSink as _;
        let event = JournalEvent {
            backend: self.journal.backend_uuid(),
            seq: 1,
            at: Timestamp::now(),
            kind: "approval_decided".to_owned(),
            data: json!({"approval_id": approval, "state": state, "decided_on": "desktop"}),
        };
        self.service.deliver(&event).await;
    }

    /// Waits until `link_progress` says the background read finished, for up to thirty seconds: a
    /// loaded machine running the whole suite can take well over the ten a quiet one needs.
    pub async fn wait_read(
        &self,
        link: ordin_workspaces::SourceLinkId,
    ) -> ordin_workspaces::LinkProgress {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            let progress = self.service.link_progress(link).await.expect("progress");
            if progress.done {
                return progress;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the background read did not finish");
    }
}

// ---- Microsoft Graph shapes (SharePointAdapter.md, Graph surface and Example)

pub fn graph(tail: &str) -> String {
    format!("/v1.0{tail}")
}

/// A file as `GET …/items/{id}` and the children listings return it.
pub fn file_json(id: &str, name: &str, ctag: &str, parent_path: &str, by: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "eTag": format!("\"{{6D1A}},{ctag}\""),
        "cTag": ctag,
        "size": 88210,
        "lastModifiedDateTime": "2026-01-10T13:40:00Z",
        "lastModifiedBy": {"user": {"displayName": by}},
        "parentReference": {
            "driveId": DRIVE,
            "siteId": SITE,
            "path": format!("/drives/{DRIVE}/root:{parent_path}"),
        },
        "file": {},
        "webUrl": format!("https://acme.sharepoint.com/sites/Procurement/Shared%20Documents{parent_path}/{name}"),
    })
}

pub fn folder_json(id: &str, name: &str, parent_path: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "eTag": "\"{F0},1\"",
        "cTag": "\"c:{F0},0\"",
        "size": 0,
        "lastModifiedDateTime": "2026-01-09T09:00:00Z",
        "parentReference": {
            "driveId": DRIVE,
            "siteId": SITE,
            "path": format!("/drives/{DRIVE}/root:{parent_path}"),
        },
        "folder": {"childCount": 3},
        "webUrl": format!("https://acme.sharepoint.com/sites/Procurement/Shared%20Documents{parent_path}/{name}"),
    })
}

/// The Suppliers/Contracts folder of the Gherkin Background.
pub fn contracts_folder() -> SharePointLocation {
    SharePointLocation {
        site_id: SITE.to_owned(),
        site_name: "Procurement".to_owned(),
        drive_id: DRIVE.to_owned(),
        drive_name: "Documents".to_owned(),
        item_id: Some(FOLDER.to_owned()),
        path: "Suppliers/Contracts".to_owned(),
        is_folder: true,
        web_url:
            "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents/Suppliers/Contracts"
                .to_owned(),
    }
}

pub async fn get_json(env: &Env, url_path: &str, status: u16, body: Value) {
    Mock::given(method("GET"))
        .and(path(url_path))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&env.mock)
        .await;
}

pub async fn status_only(env: &Env, verb: &str, url_path: &str, status: u16) {
    Mock::given(method(verb))
        .and(path(url_path))
        .respond_with(ResponseTemplate::new(status))
        .mount(&env.mock)
        .await;
}

/// The folder's own metadata, its drive and its site: what `resolve_origin` reads.
pub async fn mount_origin(env: &Env) {
    get_json(
        env,
        &graph(&format!("/drives/{DRIVE}/items/{FOLDER}")),
        200,
        folder_json(FOLDER, "Contracts", "/Suppliers"),
    )
    .await;
    get_json(
        env,
        &graph(&format!("/drives/{DRIVE}")),
        200,
        json!({"id": DRIVE, "name": "Documents", "webUrl": "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents"}),
    )
    .await;
    get_json(
        env,
        &graph(&format!("/sites/{SITE}")),
        200,
        json!({"id": SITE, "displayName": "Procurement", "webUrl": "https://acme.sharepoint.com/sites/Procurement"}),
    )
    .await;
}

pub fn delta_link(env: &Env, token: &str) -> String {
    format!(
        "{}/v1.0/drives/{DRIVE}/root/delta?token={token}",
        env.mock.uri()
    )
}

/// `GET …/root/delta?token=<query_token>` answers `entries` and the delta link `next_token`.
pub async fn mount_delta(env: &Env, query_token: &str, entries: Vec<Value>, next_token: &str) {
    Mock::given(method("GET"))
        .and(path(graph(&format!("/drives/{DRIVE}/root/delta"))))
        .and(wiremock::matchers::query_param("token", query_token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": entries,
            "@odata.deltaLink": delta_link(env, next_token),
        })))
        .mount(&env.mock)
        .await;
}

/// `GET …/items/<folder>/children` answers `items` on one page.
pub async fn mount_children(env: &Env, folder_id: &str, items: Vec<Value>) {
    get_json(
        env,
        &graph(&format!("/drives/{DRIVE}/items/{folder_id}/children")),
        200,
        json!({"value": items}),
    )
    .await;
}

/// Answers `GET …/items/{id}` for every file id of `files` with the metadata the id's cTag gives, the
/// content of each file and the version label.
struct FileMeta(Vec<(String, String, String)>);

impl Respond for FileMeta {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let id = request
            .url
            .path()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        match self.0.iter().find(|(file, _, _)| *file == id) {
            Some((file, name, ctag)) => ResponseTemplate::new(200).set_body_json(file_json(
                file,
                name,
                ctag,
                "/Suppliers/Contracts",
                "Sam Lee",
            )),
            None => ResponseTemplate::new(404),
        }
    }
}

/// `GET …/items/{folder}:/{name}`: the path lookup `item_at` makes.
struct FileByName(Vec<(String, String, String)>);

impl Respond for FileByName {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let name = request
            .url
            .path()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .replace("%20", " ");
        match self.0.iter().find(|(_, file, _)| *file == name) {
            Some((id, file, ctag)) => ResponseTemplate::new(200).set_body_json(file_json(
                id,
                file,
                ctag,
                "/Suppliers/Contracts",
                "Sam Lee",
            )),
            None => ResponseTemplate::new(404),
        }
    }
}

struct FileContent;

impl Respond for FileContent {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let id = request
            .url
            .path()
            .trim_end_matches("/content")
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        ResponseTemplate::new(200).set_body_bytes(format!("content of {id}").into_bytes())
    }
}

/// A library of files `(id, name, cTag)` under Suppliers/Contracts: the folder's own metadata, its
/// listing, each file's metadata, content and version label.
pub async fn mount_library(env: &Env, files: &[(String, String, String)]) {
    mount_origin(env).await;
    let listing: Vec<Value> = files
        .iter()
        .map(|(id, name, ctag)| file_json(id, name, ctag, "/Suppliers/Contracts", "Sam Lee"))
        .collect();
    mount_children(env, FOLDER, listing).await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/v1\.0/drives/[^/]+/items/[^/]+$"))
        .respond_with(FileMeta(files.to_vec()))
        .mount(&env.mock)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/v1\.0/drives/[^/]+/items/[^/]+:/[^/]+$"))
        .respond_with(FileByName(files.to_vec()))
        .mount(&env.mock)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/v1\.0/drives/[^/]+/items/[^/]+/content$"))
        .respond_with(FileContent)
        .mount(&env.mock)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/v1\.0/drives/[^/]+/items/[^/]+/versions$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"value": [{"id": "4.0", "lastModifiedDateTime": "2026-01-10T13:40:00Z"}]}),
        ))
        .mount(&env.mock)
        .await;
    mount_delta(env, "latest", vec![], "T1").await;
}

/// The one file of the Gherkin Background at `ctag`.
pub fn kestrel(ctag: &str) -> (String, String, String) {
    (
        FILE.to_owned(),
        "Kestrel MSA.docx".to_owned(),
        ctag.to_owned(),
    )
}

/// `n` files `doc-001.txt` … at cTag `c4`.
pub fn many_files(n: usize) -> Vec<(String, String, String)> {
    (1..=n)
        .map(|i| {
            (
                format!("F{i:04}"),
                format!("doc-{i:04}.txt"),
                "c4".to_owned(),
            )
        })
        .collect()
}

/// The delta entry that reports `Kestrel MSA.docx` at `ctag`.
pub fn kestrel_change(ctag: &str) -> Value {
    file_json(
        FILE,
        "Kestrel MSA.docx",
        ctag,
        "/Suppliers/Contracts",
        "Sam Lee",
    )
}

// ---- Confluence Cloud shapes (ConfluenceAdapter.md, surface and Example)

pub fn wiki(tail: &str) -> String {
    format!("/ex/confluence/{CLOUD}/wiki{tail}")
}

pub fn page_json(id: &str, title: &str, version: u64) -> Value {
    json!({
        "id": id,
        "status": "current",
        "title": title,
        "spaceId": SPACE_ID,
        "parentId": PARENT,
        "version": {"number": version, "authorId": "acc-1", "createdAt": "2026-01-10T13:40:00.000Z"},
        "body": {"storage": {"value": "<p>Body</p>", "representation": "storage"}},
        "_links": {"webui": format!("/spaces/PROC/pages/{id}")},
    })
}

pub async fn wiki_json(env: &Env, url_path: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(wiki(url_path)))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&env.mock)
        .await;
}

/// The parent page's restrictions: what `audience` reads.
pub async fn mount_restrictions(env: &Env) {
    wiki_json(
        env,
        &format!("/rest/api/content/{PARENT}/restriction/byOperation/read"),
        json!({"restrictions": {
            "user": {"results": []},
            "group": {"results": [{"name": "Procurement team", "id": "g-1"}]},
        }}),
    )
    .await;
}

/// Restrictions, the space and the parent page: what a link's card is built from.
pub async fn mount_where(env: &Env) {
    mount_restrictions(env).await;
    wiki_json(
        env,
        "/api/v2/spaces",
        json!({"results": [{"id": SPACE_ID, "key": "PROC", "name": "Procurement", "_links": {"webui": "/spaces/PROC"}}]}),
    )
    .await;
    wiki_json(
        env,
        &format!("/api/v2/pages/{PARENT}"),
        page_json(PARENT, "Supplier pages", 3),
    )
    .await;
}

pub fn parent_location() -> ordin_confluence::ConfluenceLocation {
    ordin_confluence::ConfluenceLocation {
        space_key: "PROC".to_owned(),
        space_name: "Procurement".to_owned(),
        page_id: Some(PARENT.to_owned()),
        title: "Supplier pages".to_owned(),
        web_url: "https://acme.atlassian.net/wiki/spaces/PROC/pages/4000".to_owned(),
    }
}

// ---- Jira Cloud shapes (JiraAdapter.md, surface and Example)

pub const JIRA_API: &str = "/rest/api/3";
pub const JIRA_UPDATED: &str = "2026-01-10T09:00:00.000+0000";

pub fn adf(text: &str) -> Value {
    json!({"version": 1, "type": "doc",
           "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}]})
}

pub fn jira_user(account_id: &str, display_name: &str) -> Value {
    json!({"accountId": account_id, "displayName": display_name, "active": true, "accountType": "atlassian"})
}

/// ABC-12 with a description, one attachment `rates.xlsx` of 12 bytes and one sub-task ABC-13.
pub fn abc12(description: &str, updated: &str) -> Value {
    json!({
        "id": "10012",
        "key": "ABC-12",
        "fields": {
            "summary": "Northwind assessment",
            "description": adf(description),
            "assignee": jira_user("5b10ac8d82e05b22cc7d4ef5", "Sam Lee"),
            "status": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}},
            "labels": ["ordin-handover"],
            "updated": updated,
            "attachment": [
                {"id": "10001", "filename": "rates.xlsx", "mimeType": "application/octet-stream",
                 "size": 11, "created": JIRA_UPDATED}
            ],
            "subtasks": [
                {"id": "10013", "key": "ABC-13",
                 "fields": {"summary": "Collect references", "status": {"statusCategory": {"key": "new"}}}}
            ],
            "issuelinks": []
        }
    })
}

pub fn jira_comment(id: &str, created: &str, body: Value) -> Value {
    json!({"id": id, "author": jira_user("5b10ac8d82e05b22cc7d4ef5", "Sam Lee"),
           "created": created, "updated": created, "body": body})
}

/// An ADF document with a paragraph and then a code block.
pub fn adf_with_code(note: &str, code: &str) -> Value {
    json!({"version": 1, "type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": note}]},
        {"type": "codeBlock", "attrs": {"language": "yaml"}, "content": [{"type": "text", "text": code}]}
    ]})
}

/// Mounts the issue read, its comment pages and the attachment content.
pub async fn mount_jira_issue(env: &Env, issue: Value, comments: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path(format!("{JIRA_API}/issue/ABC-12")))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue))
        .mount(&env.mock)
        .await;
    let total = comments.len();
    Mock::given(method("GET"))
        .and(path(format!("{JIRA_API}/issue/ABC-12/comment")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "startAt": 0, "maxResults": 100, "total": total, "comments": comments
        })))
        .mount(&env.mock)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{JIRA_API}/attachment/content/10001")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"rates bytes".to_vec()))
        .mount(&env.mock)
        .await;
}
