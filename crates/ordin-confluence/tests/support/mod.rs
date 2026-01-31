//! Shared test helpers: a Journal on a temporary SQLite file, the real Identity, Approval and File
//! services, and a `wiremock` Confluence stand-in behind the Atlassian gateway on loopback, whose replies
//! follow ConfluenceAdapter.md (*Confluence Cloud surface used*, *Example*). Real network is never used.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_confluence::{ConfluenceAdapter, ConfluenceDeps, ConfluenceSettings, WordToMarkdown};
use ordin_files::{AddPlace, FileService, FileServiceDeps, WriteVersion};
use ordin_identity::{
    IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig, SecretName,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry,
    ModuleMigrations, SystemClock, MIGRATIONS,
};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, ProgramKind, Proposal, RulesConfig,
    RulesEngine, RulesHost, RunKey,
};
use ordin_types::domain::places::{
    ConfluenceLink, ConfluenceLinkState, CopyDestination, CopyFormat, CopyMaker, CopyOriginal,
    FileVersionOrigin, Place, PlaceKind, PublishedCopy, PublishedCopyState, PublishedVia,
    SourceLink, SourceLinkKind, SourceLinkState, Workspace, WorkspaceState,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{
    BackendId, ConfluenceLinkId, ExternalActionKey, PlaceId, PublishedCopyId, RunId, SourceLinkId,
    WorkspaceId,
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::watch;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const SITE: &str = "https://acme.atlassian.net";
pub const CLOUD: &str = "cloud-1";
pub const TOKEN: &str = "at-secret-token-0001";
pub const SPACE_ID: &str = "65010";
pub const PARENT: &str = "4000";
pub const SCOPES: [&str; 12] = [
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
        let dir = std::env::temp_dir().join(format!("ordin-confluence-{}", uuid::Uuid::now_v7()));
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

/// How the person is signed in to Confluence in a test.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SignIn {
    /// A credential with every scope, a token valid for an hour and no refresh token.
    Token,
    /// As `Token`, with a refresh token and a token endpoint on the stand-in (`/token`).
    Refreshable,
    /// Every scope but the restriction read, which Atlassian refused (C-11, C-42).
    NoRestrictionScope,
    /// A credential that lacks the scopes the adapter needs.
    WrongScopes,
    /// No credential.
    Nobody,
}

pub struct Options {
    pub sign_in: SignIn,
    pub timeout: Duration,
    pub word: Option<Arc<dyn WordToMarkdown>>,
    pub site: Option<&'static str>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            sign_in: SignIn::Token,
            timeout: Duration::from_secs(5),
            word: None,
            site: Some(SITE),
        }
    }
}

pub struct Env {
    pub dir: TempDir,
    pub mock: MockServer,
    pub adapter: Arc<ConfluenceAdapter>,
    pub journal: Arc<Journal>,
    pub pool: SqlitePool,
    pub identity: Arc<IdentityService>,
    pub approvals: Arc<ApprovalService>,
    pub files: Arc<FileService>,
    pub settings: watch::Sender<ConfluenceSettings>,
    pub place: Place,
    pub workspace: Workspace,
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

/// Seeds a Confluence credential the way a completed sign-in leaves it, then starts the service.
async fn sign_in_service(
    dir: &TempDir,
    key_file: &Path,
    pool: &SqlitePool,
    mock: &MockServer,
    sign_in: SignIn,
) -> IdentityService {
    if sign_in != SignIn::Nobody {
        let seeding =
            IdentityService::new(identity_config(dir.path(), key_file), Some(pool.clone()))
                .await
                .expect("identity");
        let id = ordin_types::CredentialId::new();
        let name = format!("credential/confluence/{id}");
        let scopes: Vec<&str> = match sign_in {
            SignIn::WrongScopes => vec!["read:me"],
            SignIn::NoRestrictionScope => SCOPES
                .iter()
                .copied()
                .filter(|s| *s != "read:content.restriction:confluence")
                .collect(),
            _ => SCOPES.to_vec(),
        };
        let (refresh, endpoint, expires) = match sign_in {
            SignIn::Refreshable => (
                json!("rt-secret-refresh-0001"),
                json!(format!("{}/token", mock.uri())),
                json!(
                    Timestamp::from_unix_millis(Timestamp::now().unix_millis() + 3_600_000)
                        .unwrap()
                ),
            ),
            _ => (Value::Null, Value::Null, Value::Null),
        };
        let blob = json!({
            "access_token": TOKEN,
            "refresh_token": refresh,
            "expires_at": expires,
            "scopes": scopes,
            "token_type": "bearer",
            "client_id": "client-1",
            "token_endpoint": endpoint,
        });
        seeding
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
             VALUES (?1, 'confluence', 'acme.atlassian.net', ?2, NULL, 'active', ?3)",
        )
        .bind(id.to_string())
        .bind(&name)
        .bind(&now)
        .execute(pool)
        .await
        .expect("credential row");
        sqlx::query(
            "INSERT INTO credential_meta (credential_id, provider_host, scopes, needs_sign_in) \
             VALUES (?1, 'acme.atlassian.net', ?2, 0)",
        )
        .bind(id.to_string())
        .bind(serde_json::to_string(&scopes).expect("scopes"))
        .execute(pool)
        .await
        .expect("credential meta");
    }
    IdentityService::new(identity_config(dir.path(), key_file), Some(pool.clone()))
        .await
        .expect("identity")
}

impl Env {
    pub async fn new() -> Env {
        Env::with(Options::default()).await
    }

    pub async fn with_sign_in(sign_in: SignIn) -> Env {
        Env::with(Options {
            sign_in,
            ..Options::default()
        })
        .await
    }

    pub async fn with_timeout(timeout: Duration) -> Env {
        Env::with(Options {
            timeout,
            ..Options::default()
        })
        .await
    }

    pub async fn with(options: Options) -> Env {
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
            rules,
            agent_folders,
            data_root: data.clone(),
        })
        .await
        .expect("file service");

        let folder = dir.path().join("work");
        std::fs::create_dir_all(&folder).expect("linked folder");
        let place = files
            .add_place(AddPlace {
                path: folder,
                friendly_name: Some("Supplier quotes".to_owned()),
                kind: PlaceKind::WorkspaceFolder,
            })
            .await
            .expect("place");
        let workspace = Workspace {
            id: WorkspaceId::new(),
            name: "Supplier review".to_owned(),
            own_folder_place_id: place.id,
            state: WorkspaceState::Active,
            last_seen_at: None,
            moved_to_bin_at: None,
            created_at: Timestamp::now(),
        };

        let mock = MockServer::start().await;
        let key_file = dir.path().join("secrets.key");
        std::fs::write(&key_file, STANDARD.encode([7u8; 32])).expect("key file");
        let identity =
            Arc::new(sign_in_service(&dir, &key_file, &pool, &mock, options.sign_in).await);
        let (settings, receiver) = watch::channel(ConfluenceSettings {
            site: options.site.map(|s| url::Url::parse(s).expect("site")),
        });
        let adapter = ConfluenceAdapter::with_stand_in(
            ConfluenceDeps {
                journal: journal.clone(),
                identity: identity.clone(),
                approvals: approvals.clone(),
                files: files.clone(),
                http: reqwest::Client::new(),
                settings: receiver,
                word_to_markdown: options.word,
            },
            &mock.uri(),
            0.001,
            options.timeout,
        );
        let env = Env {
            dir,
            mock,
            adapter,
            journal,
            pool,
            identity,
            approvals,
            files,
            settings,
            place,
            workspace,
        };
        env.mount_resources(&[(CLOUD, SITE)]).await;
        env
    }

    /// `GET …/oauth/token/accessible-resources`.
    pub async fn mount_resources(&self, resources: &[(&str, &str)]) {
        let body: Vec<Value> = resources
            .iter()
            .map(|(id, url)| json!({"id": id, "url": url, "name": "acme", "scopes": []}))
            .collect();
        Mock::given(method("GET"))
            .and(path("/oauth/token/accessible-resources"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.mock)
            .await;
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

    /// As `requests`, leaving out the gateway's look-up of the site.
    pub async fn confluence_requests(&self) -> Vec<(String, String)> {
        self.requests()
            .await
            .into_iter()
            .filter(|(_, p)| p != "/oauth/token/accessible-resources")
            .collect()
    }

    pub async fn count(&self, verb: &str, url_path: &str) -> usize {
        self.requests()
            .await
            .iter()
            .filter(|(m, p)| m == verb && p == url_path)
            .count()
    }

    pub async fn writes(&self) -> Vec<(String, String)> {
        self.confluence_requests()
            .await
            .into_iter()
            .filter(|(m, _)| m != "GET")
            .collect()
    }

    /// The JSON body of the one request with this method and path.
    pub async fn body_of(&self, verb: &str, url_path: &str) -> Value {
        let requests = self.mock.received_requests().await.unwrap_or_default();
        let request = requests
            .iter()
            .find(|r| r.method.as_str() == verb && r.url.path() == url_path)
            .expect("the request was sent");
        serde_json::from_slice(&request.body).expect("a JSON body")
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

    /// (state) of every external action, in the order they were begun.
    pub async fn actions(&self) -> Vec<String> {
        use sqlx::Row as _;
        sqlx::query("SELECT state FROM external_actions ORDER BY sent_at")
            .fetch_all(&self.pool)
            .await
            .expect("actions")
            .iter()
            .map(|r| r.get::<String, _>(0))
            .collect()
    }

    pub async fn action_kinds(&self) -> Vec<String> {
        use sqlx::Row as _;
        sqlx::query("SELECT kind FROM external_actions ORDER BY sent_at")
            .fetch_all(&self.pool)
            .await
            .expect("actions")
            .iter()
            .map(|r| r.get::<String, _>(0))
            .collect()
    }

    pub fn place_id(&self) -> PlaceId {
        self.place.id
    }

    pub fn link(&self, state: ConfluenceLinkState) -> ConfluenceLink {
        ConfluenceLink {
            id: ConfluenceLinkId::new(),
            workspace_id: self.workspace.id,
            place_id: self.place.id,
            folder: String::new(),
            confluence_space: "PROC".to_owned(),
            parent_page_id: PARENT.to_owned(),
            audience: "Procurement team".to_owned(),
            approval_id: None,
            approved_at: None,
            state,
        }
    }

    /// Writes a file on disk as an app outside ordin would, with no version.
    pub fn save(&self, relative: &str, text: &str) {
        let target = PathBuf::from(&self.place.path).join(relative);
        std::fs::create_dir_all(target.parent().expect("a parent")).expect("folders");
        std::fs::write(target, text).expect("a file");
    }

    /// Writes a new version of a file through the File Service.
    pub async fn version(&self, relative: &str, text: &str, origin: FileVersionOrigin) {
        self.files
            .write_version(WriteVersion {
                place_id: self.place.id,
                path: relative.to_owned(),
                bytes: text.as_bytes().to_vec(),
                origin,
                expected_hash: None,
            })
            .await
            .expect("a version");
    }

    pub async fn hash_of(&self, relative: &str) -> String {
        self.files
            .read_file(self.place.id, relative)
            .await
            .expect("the file")
            .hash
            .0
    }

    /// A page copy that is current at destination version `pushed`, published from `hash`.
    pub async fn page_copy(
        &self,
        link: &ConfluenceLink,
        relative: &str,
        page_id: &str,
        pushed: &str,
        hash: &str,
    ) -> PublishedCopy {
        self.files
            .record_published_copy(PublishedCopy {
                id: PublishedCopyId::new(),
                original: CopyOriginal::PlaceFile {
                    place_id: self.place.id,
                    path: relative.to_owned(),
                },
                destination: CopyDestination::ConfluencePage {
                    space: "PROC".to_owned(),
                    parent_page_id: PARENT.to_owned(),
                },
                destination_item_id: Some(page_id.to_owned()),
                format: CopyFormat::Page,
                maker: CopyMaker::Ordin,
                published_via: PublishedVia::ConfluenceLink {
                    confluence_link_id: link.id,
                },
                followed_branch: None,
                keep_up_to_date: true,
                audience: link.audience.clone(),
                original_version_published: Some(hash.to_owned()),
                destination_version_pushed: Some(pushed.to_owned()),
                last_updated_at: Some(Timestamp::now()),
                state: PublishedCopyState::Current,
                failure_reason: None,
            })
            .await
            .expect("a copy")
    }

    pub async fn copy(&self, id: PublishedCopyId) -> PublishedCopy {
        self.files.published_copy(id).await.expect("the copy")
    }

    /// An active link of this place, as the Workspace Service holds it.
    pub fn source_link(&self, origin: &str) -> SourceLink {
        SourceLink {
            id: SourceLinkId::new(),
            workspace_id: self.workspace.id,
            kind: SourceLinkKind::ConfluenceLocation,
            origin_location: origin.to_owned(),
            display_name: "Supplier pages".to_owned(),
            place_id: Some(self.place.id),
            state: SourceLinkState::Active,
            added_at: Timestamp::now(),
            last_read_at: None,
        }
    }
}

pub fn key() -> ExternalActionKey {
    ExternalActionKey::new()
}

/// The path of a call through the gateway.
pub fn api(tail: &str) -> String {
    format!("/ex/confluence/{CLOUD}/wiki{tail}")
}

pub fn version_json(number: u64) -> Value {
    json!({"number": number, "authorId": "acc-1", "createdAt": "2026-01-10T13:40:00.000Z"})
}

/// A page as `GET /wiki/api/v2/pages/{id}?body-format=storage` returns it.
pub fn page_json(id: &str, title: &str, version: u64) -> Value {
    json!({
        "id": id,
        "status": "current",
        "title": title,
        "spaceId": SPACE_ID,
        "parentId": PARENT,
        "version": version_json(version),
        "body": {"storage": {"value": "<p>Body</p>", "representation": "storage"}},
        "_links": {"webui": format!("/spaces/PROC/pages/{id}")},
    })
}

pub fn attachment_json(id: &str, title: &str, version: u64, size: u64) -> Value {
    json!({
        "id": id,
        "title": title,
        "mediaType": "application/pdf",
        "fileSize": size,
        "downloadLink": format!("/download/attachments/{id}/{title}"),
        "version": version_json(version),
    })
}

pub async fn reply(env: &Env, verb: &str, url_path: &str, status: u16, body: Value) {
    Mock::given(method(verb))
        .and(path(url_path))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&env.mock)
        .await;
}

/// Answers once, then falls through to the mocks mounted after it.
pub async fn reply_once(env: &Env, verb: &str, url_path: &str, status: u16, body: Value) {
    Mock::given(method(verb))
        .and(path(url_path))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .up_to_n_times(1)
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

pub async fn mount_page(env: &Env, id: &str, title: &str, version: u64) {
    reply(
        env,
        "GET",
        &api(&format!("/api/v2/pages/{id}")),
        200,
        page_json(id, title, version),
    )
    .await;
}

pub async fn mount_space(env: &Env) {
    reply(
        env,
        "GET",
        &api("/api/v2/spaces"),
        200,
        json!({"results": [{
            "id": SPACE_ID, "key": "PROC", "name": "Procurement",
            "_links": {"webui": "/spaces/PROC"},
        }]}),
    )
    .await;
}

/// `PUT /wiki/api/v2/pages/{id}` accepted at `version`.
pub async fn mount_put(env: &Env, id: &str, title: &str, version: u64) {
    reply(
        env,
        "PUT",
        &api(&format!("/api/v2/pages/{id}")),
        200,
        page_json(id, title, version),
    )
    .await;
}

/// The attachments of a page.
pub async fn mount_attachments(env: &Env, page_id: &str, attachments: Vec<Value>) {
    reply(
        env,
        "GET",
        &api(&format!("/api/v2/pages/{page_id}/attachments")),
        200,
        json!({"results": attachments, "_links": {}}),
    )
    .await;
}

pub async fn mount_children(env: &Env, page_id: &str, children: Vec<Value>) {
    reply(
        env,
        "GET",
        &api(&format!("/api/v2/pages/{page_id}/children")),
        200,
        json!({"results": children, "_links": {}}),
    )
    .await;
}

/// `GET /wiki/rest/api/user`: any account is Sam Lee.
pub async fn mount_author(env: &Env) {
    reply(
        env,
        "GET",
        &api("/rest/api/user"),
        200,
        json!({"accountId": "acc-1", "displayName": "Sam Lee"}),
    )
    .await;
}

/// `GET /wiki/rest/api/search`.
pub async fn mount_search(env: &Env, hits: Vec<Value>) {
    reply(
        env,
        "GET",
        &api("/rest/api/search"),
        200,
        json!({"results": hits, "_links": {}}),
    )
    .await;
}

pub fn page_hit(id: &str, title: &str) -> Value {
    json!({
        "content": {"id": id, "type": "page", "title": title},
        "resultGlobalContainer": {"title": "Procurement", "displayUrl": "/spaces/PROC"},
        "url": format!("/spaces/PROC/pages/{id}"),
    })
}

pub fn space_hit() -> Value {
    json!({
        "space": {"key": "PROC", "name": "Procurement"},
        "title": "Procurement",
        "url": "/spaces/PROC",
    })
}

/// Parent-page restrictions and space lookup answered so a link's card can be built.
pub async fn mount_where(env: &Env) {
    mount_space(env).await;
    mount_page(env, PARENT, "Supplier pages", 3).await;
    reply(
        env,
        "GET",
        &api(&format!(
            "/rest/api/content/{PARENT}/restriction/byOperation/read"
        )),
        200,
        json!({"restrictions": {
            "user": {"results": []},
            "group": {"results": [{"name": "Procurement team", "id": "g-1"}]},
        }}),
    )
    .await;
}
