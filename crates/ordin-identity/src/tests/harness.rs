//! The test harness: one service with its stand-ins.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use ordin_types::domain::identity::CredentialKind;
use ordin_types::time::Timestamp;
use ordin_types::CredentialId;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use wiremock::MockServer;

use crate::config::{BrowserOpener, IdentityConfig, KeySource, Program, SecretBackendConfig};
use crate::error::CredError;
use crate::oauth::Http;
use crate::secret::{Backend, Keychain, SecretBackendKind, SecretName, SecretStore};
use crate::secret_file::EncryptedFile;
use crate::service::{new_row, IdentityService, TokenBlob};
use crate::store::Meta;
use crate::SQLITE_MIGRATIONS;

/// Browser-method tests bind the fixed loopback ports; they take this lock so they never overlap.
pub(crate) static PORTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A fresh folder under the system temporary folder.
pub(crate) fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ordin-identity-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The in-memory stand-in for the OS keychain: counts calls and can be told to refuse access.
#[derive(Default)]
pub(crate) struct MemoryKeychain {
    pub items: Mutex<HashMap<String, Vec<u8>>>,
    pub puts: AtomicUsize,
    pub gets: AtomicUsize,
    pub locked: AtomicBool,
}

impl MemoryKeychain {
    fn check(&self) -> Result<(), CredError> {
        if self.locked.load(Ordering::SeqCst) {
            // As `keyring` reports a locked keychain (`NoStorageAccess`), mapped by the real back end.
            return Err(CredError::of(crate::error::code::KEYCHAIN_UNAVAILABLE));
        }
        Ok(())
    }
}

impl Keychain for MemoryKeychain {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, CredError> {
        self.check()?;
        self.gets.fetch_add(1, Ordering::SeqCst);
        Ok(self.items.lock().unwrap().get(name).cloned())
    }
    fn set(&self, name: &str, value: &[u8]) -> Result<(), CredError> {
        self.check()?;
        self.puts.fetch_add(1, Ordering::SeqCst);
        self.items
            .lock()
            .unwrap()
            .insert(name.to_owned(), value.to_vec());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<(), CredError> {
        self.check()?;
        self.items.lock().unwrap().remove(name);
        Ok(())
    }
}

/// Records every URL the "system browser" was asked to open.
#[derive(Default)]
pub(crate) struct Browser {
    pub opened: Mutex<Vec<String>>,
    pub fail: AtomicBool,
}

impl BrowserOpener for Browser {
    fn open(&self, url: &str) -> Result<(), String> {
        if self.fail.load(Ordering::SeqCst) {
            return Err("no browser".to_owned());
        }
        self.opened.lock().unwrap().push(url.to_owned());
        Ok(())
    }
}

impl Browser {
    pub fn last(&self) -> Option<String> {
        self.opened.lock().unwrap().last().cloned()
    }
}

pub(crate) enum Store {
    Memory,
    /// The encrypted file with a key file holding base64 of 32 bytes.
    File,
}

pub(crate) struct Setup {
    pub program: Program,
    pub store: Store,
    pub browser: bool,
    pub github: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            program: Program::Desktop,
            store: Store::Memory,
            browser: true,
            github: true,
        }
    }
}

pub(crate) struct Harness {
    pub service: IdentityService,
    pub mock: MockServer,
    pub browser: Arc<Browser>,
    pub keychain: Arc<MemoryKeychain>,
    pub pool: SqlitePool,
    pub dir: PathBuf,
}

/// A migrated SQLite file in `dir`, as the program hands the Journal's pool over.
pub(crate) async fn migrated_pool(dir: &std::path::Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(dir.join(format!("journal-{}.db", uuid::Uuid::now_v7())))
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await.expect("pool");
    for (_, _, sql) in SQLITE_MIGRATIONS {
        sqlx::raw_sql(*sql).execute(&pool).await.expect("migration");
    }
    pool
}

pub(crate) fn key_file(dir: &std::path::Path, key: [u8; 32]) -> PathBuf {
    let path = dir.join("secrets.key");
    std::fs::write(&path, STANDARD.encode(key)).expect("key file");
    path
}

impl Harness {
    pub async fn new() -> Self {
        Self::with(Setup::default()).await
    }

    pub async fn with(setup: Setup) -> Self {
        let dir = temp_dir();
        let pool = migrated_pool(&dir).await;
        let mock = MockServer::start().await;
        let browser = Arc::new(Browser::default());
        let keychain = Arc::new(MemoryKeychain::default());
        let service = Self::build(&setup, &dir, pool.clone(), &mock, &browser, &keychain).await;
        Harness {
            service,
            mock,
            browser,
            keychain,
            pool,
            dir,
        }
    }

    async fn build(
        setup: &Setup,
        dir: &std::path::Path,
        pool: SqlitePool,
        mock: &MockServer,
        browser: &Arc<Browser>,
        keychain: &Arc<MemoryKeychain>,
    ) -> IdentityService {
        let (secret_backend, store) = match setup.store {
            Store::Memory => (
                // The OS keychain is the desktop's back end; a server's keychain is the Secret Service.
                if setup.program == Program::Desktop {
                    SecretBackendConfig::OsKeychain
                } else {
                    SecretBackendConfig::SecretService
                },
                SecretStore::from_backend(Backend::Keychain {
                    kind: SecretBackendKind::OsKeychain,
                    keychain: Arc::clone(keychain) as Arc<dyn Keychain>,
                }),
            ),
            Store::File => {
                let source = KeySource::KeyFile(key_file(dir, [7u8; 32]));
                let file = EncryptedFile::open(dir, &source).await.expect("file store");
                (
                    SecretBackendConfig::EncryptedFile { key_source: source },
                    SecretStore::from_backend(Backend::File(file)),
                )
            }
        };
        let config = IdentityConfig {
            program: setup.program,
            data_root: dir.to_path_buf(),
            keychain_service: "io.gatblau.ordin.test".to_owned(),
            secret_backend,
            github_app_client_id: setup.github.then(|| "Iv1.ordin-test".to_owned()),
            http: reqwest::Client::new(),
            browser: setup
                .browser
                .then(|| Arc::clone(browser) as Arc<dyn BrowserOpener>),
        };
        let http = Http {
            client: reqwest::Client::new(),
            timeout: std::time::Duration::from_secs(2),
            redirect: Some(mock.uri()),
        };
        crate::config::check_config(&config, true).expect("config");
        match IdentityService::start(config, Some(pool), store, http).await {
            Ok(service) => service,
            Err(e) => panic!("start: {e}"),
        }
    }

    /// Writes a credential as a completed sign-in would (C.10), for tests that start signed in.
    pub async fn seed(&self, kind: CredentialKind, label: &str, seed: Seed) -> CredentialId {
        let id = CredentialId::new();
        let blob = TokenBlob {
            access_token: seed.access_token.to_owned(),
            refresh_token: seed.refresh_token.map(str::to_owned),
            expires_at: seed.expires_at,
            scopes: seed.scopes.iter().map(|s| (*s).to_owned()).collect(),
            token_type: "bearer".to_owned(),
            method: seed.method.map(str::to_owned),
            email: seed.email.map(str::to_owned),
            account_id: None,
            client_id: Some(seed.client_id.to_owned()),
            token_endpoint: Some(seed.token_endpoint.to_owned()),
            client_secret: None,
        };
        let meta = Meta {
            provider_host: seed.provider_host.to_owned(),
            scopes: seed.scopes.iter().map(|s| (*s).to_owned()).collect(),
            needs_sign_in: false,
            refused_at: None,
            last_error_code: None,
            display_name: seed.display_name.map(str::to_owned),
        };
        let row = new_row(
            id,
            kind,
            label.to_owned(),
            Timestamp::now(),
            seed.expires_at,
            meta,
        );
        let bytes = serde_json::to_vec(&blob).unwrap();
        self.service
            .inner
            .persist(row, true, &bytes, None)
            .await
            .expect("seed");
        id
    }

    /// The stored token blob of a credential, read straight from the secret store.
    pub async fn blob(&self, kind: CredentialKind, id: CredentialId) -> Option<serde_json::Value> {
        let name = SecretName::new(&crate::service::credential_secret_name(kind, id)).unwrap();
        let bytes = self.service.secrets().get(&name).await.unwrap()?;
        Some(serde_json::from_slice(&bytes).unwrap())
    }

    /// Every column value of every row of both tables, as text.
    pub async fn table_text(&self) -> String {
        use sqlx::Row as _;
        let mut out = String::new();
        for table in ["credentials", "credential_meta"] {
            let sql: &'static str = if table == "credentials" {
                "SELECT * FROM credentials"
            } else {
                "SELECT * FROM credential_meta"
            };
            for row in sqlx::query(sql).fetch_all(&self.pool).await.unwrap() {
                for i in 0..row.len() {
                    if let Ok(Some(v)) = row.try_get::<Option<String>, _>(i) {
                        out.push_str(&v);
                        out.push('\n');
                    }
                }
            }
        }
        out
    }

    pub async fn count(&self, sql: &'static str) -> i64 {
        use sqlx::Row as _;
        sqlx::query(sql)
            .fetch_one(&self.pool)
            .await
            .unwrap()
            .get::<i64, _>(0)
    }
}

/// The values of a seeded credential.
pub(crate) struct Seed {
    pub access_token: &'static str,
    pub refresh_token: Option<&'static str>,
    pub expires_at: Option<Timestamp>,
    pub scopes: &'static [&'static str],
    pub client_id: &'static str,
    pub token_endpoint: &'static str,
    pub provider_host: &'static str,
    pub display_name: Option<&'static str>,
    pub method: Option<&'static str>,
    pub email: Option<&'static str>,
}

impl Default for Seed {
    fn default() -> Self {
        Seed {
            access_token: "at-seeded-0001",
            refresh_token: Some("rt-seeded-0001"),
            expires_at: Some(in_minutes(60)),
            scopes: &[],
            client_id: "client-1",
            token_endpoint: "https://provider.example/token",
            provider_host: "provider.example",
            display_name: None,
            method: None,
            email: None,
        }
    }
}

pub(crate) fn in_minutes(minutes: i64) -> Timestamp {
    Timestamp::from_unix_millis(Timestamp::now().unix_millis() + minutes * 60_000).unwrap()
}

/// An unsigned ID token carrying `claims` (C.8 reads its payload only).
pub(crate) fn id_token(claims: serde_json::Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
    format!("{header}.{payload}.sig")
}

/// The query parameters of a URL.
pub(crate) fn query(url: &str) -> HashMap<String, String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Plays the browser: follows the provider's redirect to the loopback listener.
pub(crate) async fn redirect(authorisation_url: &str, params: &[(&str, &str)]) -> u16 {
    let q = query(authorisation_url);
    let mut target = url::Url::parse(&q["redirect_uri"]).unwrap();
    {
        let mut pairs = target.query_pairs_mut();
        for (k, v) in params {
            pairs.append_pair(k, v);
        }
    }
    reqwest::get(target).await.unwrap().status().as_u16()
}

/// A log capture with ordin's own line formatter (Logging.md), for "never logged" assertions.
#[derive(Clone, Default)]
pub(crate) struct Capture(pub Arc<Mutex<Vec<u8>>>);

impl Capture {
    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    pub fn install(&self) -> tracing::subscriber::DefaultGuard {
        let writer = self.clone();
        let subscriber = tracing_subscriber::fmt()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }
}

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The code of an error result.
pub(crate) fn code_of<T>(result: Result<T, CredError>) -> &'static str {
    use ordin_types::CodedError as _;
    match result {
        Ok(_) => "ok",
        Err(e) => e.code(),
    }
}
