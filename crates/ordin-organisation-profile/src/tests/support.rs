//! Shared test fixtures: a temporary data root with a real SQLite journal file, the signing key K1, a
//! `wiremock` stand-in for the company's profile address on loopback, a settable clock and a log
//! capture through the workspace's own line formatter.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use ordin_types::encoding::b64url_encode;
use ordin_types::time::Timestamp;
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::jws::kid_of;
use crate::{
    Clock, InstallerSettings, OrganisationProfileService, ProfileMode, ProfileOptions,
    SQLITE_MIGRATIONS,
};

/// A clock the test moves.
pub(crate) struct TestClock(Mutex<Timestamp>);

impl TestClock {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(TestClock(Mutex::new(Timestamp::now())))
    }
    pub(crate) fn advance(&self, by: Duration) {
        let mut now = self.0.lock().unwrap();
        let ms = now.unix_millis() + by.as_millis() as i64;
        *now = Timestamp::from_unix_millis(ms).unwrap();
    }
}

impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        *self.0.lock().unwrap()
    }
}

/// A data root under the system temporary folder, removed on drop.
pub(crate) struct TempRoot(pub(crate) PathBuf);

impl TempRoot {
    pub(crate) fn new() -> Self {
        Lines::install();
        let dir = std::env::temp_dir().join(format!(
            "ordin-organisation-profile-{}",
            ordin_types::OrganisationProfileId::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempRoot(dir)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A SQLite journal file under `root` with this crate's migrations applied, as the program's
/// migration registry would apply them.
pub(crate) async fn migrated_pool(root: &Path) -> SqlitePool {
    let pool = empty_pool(root).await;
    for (_, _, sql) in SQLITE_MIGRATIONS {
        sqlx::raw_sql(*sql).execute(&pool).await.unwrap();
    }
    pool
}

/// A SQLite journal file with no table of this crate.
pub(crate) async fn empty_pool(root: &Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(root.join("journal.db"))
        .create_if_missing(true);
    SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap()
}

/// An Ed25519 signing key and its installer form.
pub(crate) struct Key(SigningKey);

impl Key {
    pub(crate) fn k1() -> Self {
        Key(SigningKey::from_bytes(&[1u8; 32]))
    }
    pub(crate) fn k2() -> Self {
        Key(SigningKey::from_bytes(&[2u8; 32]))
    }
    /// base64url (no padding) of the raw 32-byte public key, as `installer.json` carries it.
    pub(crate) fn public(&self) -> String {
        b64url_encode(self.0.verifying_key().as_bytes())
    }
    pub(crate) fn kid(&self) -> String {
        kid_of(self.0.verifying_key().as_bytes())
    }
    /// The detached JWS of `document`, with the key's `kid` in the header.
    pub(crate) fn sign(&self, document: &[u8]) -> String {
        let header =
            b64url_encode(format!(r#"{{"alg":"EdDSA","kid":"{}"}}"#, self.kid()).as_bytes());
        self.sign_with_header(&header, document)
    }
    pub(crate) fn sign_with_header(&self, header_segment: &str, document: &[u8]) -> String {
        let input = format!("{header_segment}.{}", b64url_encode(document));
        let signature = self.0.sign(input.as_bytes());
        format!("{header_segment}..{}", b64url_encode(&signature.to_bytes()))
    }
}

/// A document of `version` with `extra` merged in at the top level.
pub(crate) fn document(version: &str, extra: Value) -> Vec<u8> {
    let mut base = json!({"format_version": 1, "profile_version": version});
    if let (Some(base), Value::Object(extra)) = (base.as_object_mut(), extra) {
        base.extend(extra);
    }
    serde_json::to_vec(&base).unwrap()
}

/// The company's profile address, served by `wiremock` on loopback.
pub(crate) struct Site {
    pub(crate) server: MockServer,
}

impl Site {
    pub(crate) async fn new() -> Self {
        Site {
            server: MockServer::start().await,
        }
    }
    pub(crate) fn url(&self) -> String {
        format!("{}/profile.json", self.server.uri())
    }
    /// Serves `document` and its signature by `key`, replacing whatever was served before.
    pub(crate) async fn serve(&self, document: &[u8], key: &Key) {
        self.serve_raw(document, &key.sign(document)).await;
    }
    pub(crate) async fn serve_raw(&self, document: &[u8], jws: &str) {
        self.server.reset().await;
        Mock::given(method("GET"))
            .and(path("/profile.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(document.to_vec()))
            .mount(&self.server)
            .await;
        Mock::given(method("GET"))
            .and(path("/profile.json.jws"))
            .respond_with(ResponseTemplate::new(200).set_body_string(jws))
            .mount(&self.server)
            .await;
    }
    pub(crate) async fn requests(&self) -> usize {
        self.server.received_requests().await.map_or(0, |r| r.len())
    }
}

/// Writes `installer.json` naming `url` and `keys`.
pub(crate) fn write_installer(root: &Path, url: &str, keys: &[&Key]) {
    let settings = InstallerSettings {
        profile_url: Some(url.to_owned()),
        profile_signing_keys: keys.iter().map(|k| k.public()).collect(),
        update_feed_url: None,
    };
    std::fs::write(
        root.join("installer.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
}

/// A running desktop service over `root` and `pool`.
pub(crate) async fn start(
    root: &Path,
    pool: &SqlitePool,
    clock: Arc<TestClock>,
) -> OrganisationProfileService {
    start_with_defaults(root, pool, clock, InstallerSettings::default())
        .await
        .unwrap()
}

pub(crate) async fn start_with_defaults(
    root: &Path,
    pool: &SqlitePool,
    clock: Arc<TestClock>,
    build_defaults: InstallerSettings,
) -> Result<OrganisationProfileService, crate::ProfileError> {
    OrganisationProfileService::start(
        ProfileMode::Read {
            pool: pool.clone(),
            data_root: root.to_path_buf(),
            http: reqwest::Client::new(),
            build_defaults,
        },
        ProfileOptions {
            refresh_interval: Duration::from_secs(3600),
            clock,
        },
    )
    .await
}

/// Waits until the refresh loop's first attempt has finished.
pub(crate) async fn settle(service: &OrganisationProfileService) {
    for _ in 0..6000 {
        if service.status().last_attempt_at.is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the first refresh did not finish");
}

/// The states of every `organisation_profiles` row, oldest fetch first.
pub(crate) async fn row_states(pool: &SqlitePool) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT profile_version, state, refusal_reason FROM organisation_profiles ORDER BY fetched_at, rowid",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// A started service with version `1.0.0` of `extra` active, and its site.
pub(crate) struct Running {
    pub(crate) root: TempRoot,
    pub(crate) pool: SqlitePool,
    pub(crate) site: Site,
    pub(crate) clock: Arc<TestClock>,
    pub(crate) service: OrganisationProfileService,
}

pub(crate) async fn running_with(version: &str, extra: Value) -> Running {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    let site = Site::new().await;
    site.serve(&document(version, extra), &Key::k1()).await;
    write_installer(root.path(), &site.url(), &[&Key::k1()]);
    let clock = TestClock::new();
    let service = start(root.path(), &pool, clock.clone()).await;
    settle(&service).await;
    Running {
        root,
        pool,
        site,
        clock,
        service,
    }
}

/// Serves a validly signed `version` with `extra` and refreshes once.
pub(crate) async fn refresh_with(
    running: &Running,
    version: &str,
    extra: Value,
) -> crate::RefreshOutcome {
    running
        .site
        .serve(&document(version, extra), &Key::k1())
        .await;
    running.clock.advance(Duration::from_secs(1));
    running.service.refresh().await.unwrap()
}

/// Log lines written through `ordin_types::log::JsonLineFormat`. One global subscriber serves the whole
/// test binary (a per-test scoped subscriber can miss events whose call-site interest another thread
/// cached first); it writes into the buffer the current thread registered, if any.
#[derive(Clone, Default)]
pub(crate) struct Lines(Arc<Mutex<Vec<u8>>>);

thread_local! {
    static CAPTURE: std::cell::RefCell<Option<Lines>> = const { std::cell::RefCell::new(None) };
}

/// Routes one formatted line to the calling thread's buffer.
#[derive(Clone, Copy, Default)]
pub(crate) struct ThreadWriter;

impl std::io::Write for ThreadWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        CAPTURE.with(|c| {
            if let Some(lines) = c.borrow().as_ref() {
                lines.0.lock().unwrap().extend_from_slice(bytes);
            }
        });
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ThreadWriter {
    type Writer = ThreadWriter;
    fn make_writer(&'a self) -> ThreadWriter {
        ThreadWriter
    }
}

/// Stops the capture of the current thread when dropped.
pub(crate) struct CaptureGuard;

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        CAPTURE.with(|c| c.borrow_mut().take());
    }
}

impl Lines {
    /// Installs the global subscriber once per test binary.
    pub(crate) fn install() {
        static INSTALLED: std::sync::Once = std::sync::Once::new();
        INSTALLED.call_once(|| {
            use tracing_subscriber::layer::SubscriberExt as _;
            let subscriber = tracing_subscriber::registry().with(
                tracing_subscriber::fmt::layer()
                    .event_format(ordin_types::log::JsonLineFormat)
                    .with_writer(ThreadWriter),
            );
            tracing::subscriber::set_global_default(subscriber).unwrap();
        });
    }

    pub(crate) fn capture() -> (Lines, CaptureGuard) {
        Lines::install();
        let lines = Lines::default();
        CAPTURE.with(|c| *c.borrow_mut() = Some(lines.clone()));
        (lines, CaptureGuard)
    }
    /// Every line as JSON.
    pub(crate) fn lines(&self) -> Vec<Value> {
        let text = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    /// The lines of one event.
    pub(crate) fn event(&self, name: &str) -> Vec<Value> {
        self.lines()
            .into_iter()
            .filter(|l| l["event"] == name)
            .collect()
    }
}
