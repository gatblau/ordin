//! Shared test helpers: a Journal on a temporary SQLite file, a real Identity service holding a Microsoft
//! credential, and a `wiremock` Microsoft Graph stand-in on loopback whose replies follow the shapes in
//! SharePointAdapter.md (*Microsoft Graph surface used*, *Example*, Data Model). Real network is never used.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ordin_identity::{
    IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig, SecretName,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry,
    ModuleMigrations, SystemClock, MIGRATIONS,
};
use ordin_sharepoint::{
    ExternalWrite, SharePointAdapter, SharePointDeps, SharePointItemRef, SharePointLocation,
};
use ordin_types::domain::places::{SourceLink, SourceLinkKind, SourceLinkState};
use ordin_types::domain::runs::ExternalActionKind;
use ordin_types::time::Timestamp;
use ordin_types::{ExternalActionKey, SourceLinkId, WorkspaceId};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const SITE: &str = "acme.sharepoint.com,1f0c,9a2e";
pub const DRIVE: &str = "b!x9Fq";
pub const FOLDER: &str = "01ABCDEF2GHIJ";
pub const FILE: &str = "01ABCDEF7KLMN";
pub const TOKEN: &str = "at-secret-token-0001";
pub const SCOPES: [&str; 2] = [
    "https://graph.microsoft.com/Files.ReadWrite.All",
    "https://graph.microsoft.com/Sites.Read.All",
];

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-sharepoint-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
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

/// How the person is signed in to Microsoft in a test.
pub enum SignIn {
    /// A credential with both scopes, a token valid for an hour and no refresh token.
    Token,
    /// A credential with both scopes, a refresh token and a token endpoint on the stand-in (`/token`).
    Refreshable,
    /// A credential that lacks the scopes the adapter needs.
    WrongScopes,
    /// A credential whose token endpoint answers 503, with a token about to expire.
    ProviderDown,
    /// No credential.
    Nobody,
}

pub struct Env {
    pub dir: TempDir,
    pub mock: MockServer,
    pub adapter: SharePointAdapter,
    pub journal: Journal,
    pub pool: SqlitePool,
    pub identity: Arc<IdentityService>,
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
        microsoft_client_id: None,
        http: reqwest::Client::new(),
        browser: None,
    }
}

impl Env {
    pub async fn new() -> Env {
        Env::with(SignIn::Token).await
    }

    pub async fn with(sign_in: SignIn) -> Env {
        Env::build(sign_in, 0.001, Duration::from_secs(1)).await
    }

    /// A stand-in with a longer transfer timeout, for the large uploads.
    pub async fn with_timeout(sign_in: SignIn, transfer_timeout: Duration) -> Env {
        Env::build(sign_in, 0.001, transfer_timeout).await
    }

    /// A stand-in whose waits are the spec's own (scale 1.0 is too slow for a test; 0.01 keeps the ratio).
    pub async fn with_scale(sign_in: SignIn, wait_scale: f64) -> Env {
        Env::build(sign_in, wait_scale, Duration::from_secs(1)).await
    }

    async fn build(sign_in: SignIn, wait_scale: f64, transfer_timeout: Duration) -> Env {
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
            .register(ordin_sharepoint::migrations())
            .expect("sharepoint");
        registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .expect("apply the migrations");
        let pool = store.sqlite().expect("sqlite").clone();
        let journal = Journal::start(
            store,
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("start the journal");

        let mock = MockServer::start().await;
        let key_file = dir.path().join("secrets.key");
        std::fs::write(&key_file, STANDARD.encode([7u8; 32])).expect("key file");
        let identity = Arc::new(sign_in_service(&dir, &key_file, &pool, &mock, sign_in).await);

        let adapter = SharePointAdapter::new(SharePointDeps {
            identity: Arc::clone(&identity),
            journal: journal.clone(),
            http: reqwest::Client::new(),
        })
        .expect("adapter")
        .with_stand_in(
            &format!("{}/v1.0", mock.uri()),
            wait_scale,
            transfer_timeout,
        )
        .expect("stand-in");
        Env {
            dir,
            mock,
            adapter,
            journal,
            pool,
            identity,
        }
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

    pub async fn count(&self, verb: &str, url_path: &str) -> usize {
        self.requests()
            .await
            .iter()
            .filter(|(m, p)| m == verb && p == url_path)
            .count()
    }

    /// A row count of one of this crate's tables (the table name is a literal of the test).
    pub async fn rows(&self, table: &'static str) -> i64 {
        use sqlx::Row as _;
        let sql: &'static str = match table {
            "sharepoint_link_state" => "SELECT count(*) FROM sharepoint_link_state",
            "sharepoint_items" => "SELECT count(*) FROM sharepoint_items",
            "sharepoint_origin_changes" => "SELECT count(*) FROM sharepoint_origin_changes",
            _ => "SELECT count(*) FROM external_actions",
        };
        sqlx::query(sql)
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .get::<i64, _>(0)
    }
}

/// Seeds a Microsoft credential the way a completed sign-in leaves it: the token blob in the secret
/// store, the two rows in the journal database; then starts the service that reads them.
async fn sign_in_service(
    dir: &TempDir,
    key_file: &Path,
    pool: &SqlitePool,
    mock: &MockServer,
    sign_in: SignIn,
) -> IdentityService {
    if !matches!(sign_in, SignIn::Nobody) {
        let seeding =
            IdentityService::new(identity_config(dir.path(), key_file), Some(pool.clone()))
                .await
                .expect("identity");
        let id = ordin_types::CredentialId::new();
        let name = format!("credential/microsoft/{id}");
        let scopes: Vec<&str> = match sign_in {
            SignIn::WrongScopes => vec!["User.Read"],
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
            SignIn::ProviderDown => (
                json!("rt-secret-refresh-0001"),
                json!(format!("{}/token-down", mock.uri())),
                json!(
                    Timestamp::from_unix_millis(Timestamp::now().unix_millis() + 60_000).unwrap()
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
             VALUES (?1, 'microsoft', 'sam@acme.example', ?2, NULL, 'active', ?3)",
        )
        .bind(id.to_string())
        .bind(&name)
        .bind(&now)
        .execute(pool)
        .await
        .expect("credential row");
        sqlx::query(
            "INSERT INTO credential_meta (credential_id, provider_host, scopes, needs_sign_in) \
             VALUES (?1, 'login.microsoftonline.com', ?2, 0)",
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

// ---- Graph shapes (SharePointAdapter.md, Graph surface and Example)

pub fn api(tail: &str) -> String {
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

/// The tag Graph would give a file's eTag for `ctag` in `file_json`.
pub fn etag_of(ctag: &str) -> String {
    format!("\"{{6D1A}},{ctag}\"")
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

pub fn file_ref() -> SharePointItemRef {
    SharePointItemRef {
        drive_id: DRIVE.to_owned(),
        item_id: FILE.to_owned(),
    }
}

pub fn write(kind: ExternalActionKind) -> ExternalWrite {
    ExternalWrite {
        key: ExternalActionKey::new(),
        run_id: None,
        kind,
    }
}

pub fn link_for(loc: &SharePointLocation) -> SourceLink {
    SourceLink {
        id: SourceLinkId::new(),
        workspace_id: WorkspaceId::new(),
        kind: SourceLinkKind::SharepointLocation,
        origin_location: SharePointAdapter::origin_location(loc),
        display_name: "Suppliers/Contracts".to_owned(),
        place_id: None,
        state: SourceLinkState::Active,
        added_at: Timestamp::now(),
        last_read_at: None,
    }
}

// ---- Mount helpers

pub async fn get_json(env: &Env, url_path: &str, status: u16, body: Value) {
    Mock::given(method("GET"))
        .and(path(url_path))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&env.mock)
        .await;
}

/// Answers the first request only; mount the next answer after it.
pub async fn get_json_once(env: &Env, url_path: &str, status: u16, body: Value) {
    Mock::given(method("GET"))
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

/// The metadata, the version label, the drive and the site that `upload_version` and `delete` read for a
/// file whose cTag is `ctag`.
pub async fn mount_file(env: &Env, ctag: &str) {
    get_json(
        env,
        &api(&format!("/drives/{DRIVE}/items/{FILE}")),
        200,
        file_json(
            FILE,
            "Kestrel MSA.docx",
            ctag,
            "/Suppliers/Contracts",
            "Sam Lee",
        ),
    )
    .await;
    mount_labels_and_site(env).await;
}

pub async fn mount_labels_and_site(env: &Env) {
    get_json(
        env,
        &api(&format!("/drives/{DRIVE}/items/{FILE}/versions")),
        200,
        json!({"value": [{"id": "4.0", "lastModifiedDateTime": "2026-01-10T13:40:00Z"}]}),
    )
    .await;
    get_json(
        env,
        &api(&format!("/drives/{DRIVE}")),
        200,
        json!({"id": DRIVE, "name": "Documents", "webUrl": "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents"}),
    )
    .await;
    get_json(
        env,
        &api(&format!("/sites/{SITE}")),
        200,
        json!({"id": SITE, "displayName": "Procurement", "webUrl": "https://acme.sharepoint.com/sites/Procurement"}),
    )
    .await;
}

/// Bearer-checked mock: only a request carrying the seeded token matches.
pub async fn get_with_token(env: &Env, url_path: &str, token: &str, status: u16, body: Value) {
    Mock::given(method("GET"))
        .and(path(url_path))
        .and(header("authorization", format!("Bearer {token}").as_str()))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&env.mock)
        .await;
}

// ---- Change-tracking mounts

pub fn delta_link(env: &Env, token: &str) -> String {
    format!(
        "{}/v1.0/drives/{DRIVE}/root/delta?token={token}",
        env.mock.uri()
    )
}

/// `GET …/root/delta?token=<query_token>` answers `entries` and the delta link `next_token`.
pub async fn mount_delta(
    env: &Env,
    query_token: &str,
    entries: Vec<Value>,
    next_token: &str,
    once: bool,
) {
    let mut mock = Mock::given(method("GET"))
        .and(path(api(&format!("/drives/{DRIVE}/root/delta"))))
        .and(wiremock::matchers::query_param("token", query_token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": entries,
            "@odata.deltaLink": delta_link(env, next_token),
        })));
    if once {
        mock = mock.up_to_n_times(1);
    }
    mock.mount(&env.mock).await;
}

/// `GET …/items/<folder>/children` answers `items` on one page.
pub async fn mount_children(env: &Env, folder_id: &str, items: Vec<Value>, once: bool) {
    let mut mock = Mock::given(method("GET"))
        .and(path(api(&format!(
            "/drives/{DRIVE}/items/{folder_id}/children"
        ))))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": items})));
    if once {
        mock = mock.up_to_n_times(1);
    }
    mock.mount(&env.mock).await;
}

/// The folder's own metadata, its drive and its site: what `resolve_origin` reads.
pub async fn mount_origin(env: &Env) {
    get_json(
        env,
        &api(&format!("/drives/{DRIVE}/items/{FOLDER}")),
        200,
        folder_json(FOLDER, "Contracts", "/Suppliers"),
    )
    .await;
    get_json(
        env,
        &api(&format!("/drives/{DRIVE}")),
        200,
        json!({"id": DRIVE, "name": "Documents", "webUrl": "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents"}),
    )
    .await;
    get_json(
        env,
        &api(&format!("/sites/{SITE}")),
        200,
        json!({"id": SITE, "displayName": "Procurement", "webUrl": "https://acme.sharepoint.com/sites/Procurement"}),
    )
    .await;
}

/// Tracks the Suppliers/Contracts link whose first walk finds `Kestrel MSA.docx` at `ctag`.
pub async fn track_contracts(env: &Env, ctag: &str) -> SourceLink {
    mount_origin(env).await;
    mount_delta(env, "latest", vec![], "T1", true).await;
    mount_children(
        env,
        FOLDER,
        vec![file_json(
            FILE,
            "Kestrel MSA.docx",
            ctag,
            "/Suppliers/Contracts",
            "Sam Lee",
        )],
        true,
    )
    .await;
    let link = link_for(&contracts_folder());
    env.adapter.track_link(&link).await.expect("track the link");
    link
}

/// Matches the whole `If-Match` header. `wiremock`'s `header` matcher splits a value on commas, and an
/// eTag such as `"{6D1A},4"` holds one.
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
