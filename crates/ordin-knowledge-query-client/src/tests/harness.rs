//! The test harness: a client over a real SQLite Journal, an Identity double, and knowledge agents
//! standing on loopback. An agent's address (`https://kfd.example.org`) is mapped to its loopback
//! stand-in inside the transport, so the tables hold only https addresses.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ordin_identity::{AccessToken, CredError, SecretString, SignInChanged};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_types::domain::agent_profile::{KnowledgeProxy, ServiceIdentity};
use ordin_types::domain::identity::CredentialKind;
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::wire::knowledge_query::{KnowledgeHit, KnowledgeHitKind, PageStatus, PageTier};
use ordin_types::{CredentialId, WorkspaceId};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::client::Timings;
use crate::tokens::{LinkCredential, TokenSource};
use crate::*;

pub(crate) const ISSUER: &str = "https://id.example.org";
pub(crate) const CLIENT_ID: &str = "ordin-desktop";
pub(crate) const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

pub(crate) struct TempDir(pub std::path::PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-kq-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How the Identity double answers a request for a company token.
#[derive(Clone, Copy)]
pub(crate) enum AudienceMode {
    Ok,
    Permanent,
    Retryable,
}

/// The Identity double.
pub(crate) struct FakeTokens {
    pub audience_mode: Mutex<AudienceMode>,
    pub generation: AtomicUsize,
    pub invalidated: AtomicUsize,
    pub credentials: Mutex<Vec<LinkCredential>>,
    pub confluence_ok: Mutex<bool>,
    pub scope_ok: Mutex<bool>,
    pub scope_calls: Mutex<Vec<String>>,
    pub changes: broadcast::Sender<SignInChanged>,
}

impl FakeTokens {
    pub fn new() -> Arc<FakeTokens> {
        let (changes, _) = broadcast::channel(4);
        Arc::new(FakeTokens {
            audience_mode: Mutex::new(AudienceMode::Ok),
            generation: AtomicUsize::new(1),
            invalidated: AtomicUsize::new(0),
            credentials: Mutex::new(Vec::new()),
            confluence_ok: Mutex::new(true),
            scope_ok: Mutex::new(true),
            scope_calls: Mutex::new(Vec::new()),
            changes,
        })
    }

    /// The bearer text the double issues for an audience at the current generation.
    pub fn bearer(&self, audience: &str) -> String {
        format!(
            "company-{audience}-{}",
            self.generation.load(Ordering::SeqCst)
        )
    }

    pub fn set_credentials(&self, list: Vec<LinkCredential>) {
        *self.credentials.lock().unwrap() = list;
    }
}

pub(crate) fn credential(kind: CredentialKind, host: &str) -> LinkCredential {
    LinkCredential {
        id: CredentialId::new(),
        kind,
        host: host.to_owned(),
        needs_sign_in: false,
    }
}

fn access(token: String) -> AccessToken {
    AccessToken {
        token: SecretString::new(token),
        expires_at: None,
        credential_id: CredentialId::new(),
    }
}

#[async_trait]
impl TokenSource for FakeTokens {
    async fn audience_token(
        &self,
        _issuer: &str,
        _client_id: &str,
        audience: &str,
    ) -> Result<AccessToken, CredError> {
        match *self.audience_mode.lock().unwrap() {
            AudienceMode::Ok => Ok(access(self.bearer(audience))),
            AudienceMode::Permanent => Err(CredError::Permanent {
                code: "CRED_SIGN_IN_REQUIRED",
            }),
            AudienceMode::Retryable => Err(CredError::Retryable {
                code: "CRED_PROVIDER_UNAVAILABLE",
            }),
        }
    }

    async fn invalidate_audience(&self, _issuer: &str, _client_id: &str, _audience: &str) {
        self.invalidated.fetch_add(1, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    async fn credentials(&self) -> Result<Vec<LinkCredential>, CredError> {
        Ok(self.credentials.lock().unwrap().clone())
    }

    async fn token_by_id(&self, id: CredentialId) -> Result<AccessToken, CredError> {
        Ok(access(format!("source-token-{id}")))
    }

    async fn confluence_read_me(&self) -> Result<AccessToken, CredError> {
        if *self.confluence_ok.lock().unwrap() {
            Ok(access("confluence-token".to_owned()))
        } else {
            Err(CredError::Permanent {
                code: "CRED_SCOPE_MISSING",
            })
        }
    }

    async fn scope_token(
        &self,
        _kind: CredentialKind,
        scope: &str,
    ) -> Result<AccessToken, CredError> {
        self.scope_calls.lock().unwrap().push(scope.to_owned());
        if *self.scope_ok.lock().unwrap() {
            Ok(access(format!("assertion-for-{scope}")))
        } else {
            Err(CredError::Permanent {
                code: "CRED_SCOPE_MISSING",
            })
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<SignInChanged> {
        self.changes.subscribe()
    }
}

/// Fast timings: a short hedge delay and back-off, so reconnect tests stay quick.
pub(crate) fn quick_timings() -> Timings {
    Timings {
        hedge_delay: Duration::from_millis(300),
        backoff: vec![Duration::from_millis(30), Duration::from_millis(60)],
        refresh_interval: Duration::from_secs(3600),
        about_timeout: Duration::from_secs(5),
        network_allowance: Duration::from_millis(50),
    }
}

pub(crate) async fn open_journal(dir: &TempDir) -> Arc<Journal> {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("data").join("journal.db"),
        backup_dir: dir.0.join("data").join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("register the journal");
    registry
        .register(migrations())
        .expect("register this module");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the migrations");
    Arc::new(
        Journal::start(
            store,
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("start the journal"),
    )
}

/// One knowledge agent on loopback, reachable at `https://<host>` inside the client.
pub(crate) struct Agent {
    pub mock: MockServer,
}

pub(crate) struct Fixture {
    pub client: KnowledgeQueryClient,
    pub journal: Arc<Journal>,
    pub tokens: Arc<FakeTokens>,
    pub ws: WorkspaceId,
    pub _dir: TempDir,
}

pub(crate) fn proxy(name: &str) -> KnowledgeProxy {
    KnowledgeProxy {
        name: name.to_owned(),
        url: format!("https://{name}.example.org"),
        audience: format!("aud-{name}"),
    }
}

pub(crate) fn service_settings() -> ServiceTokenSettings {
    ServiceTokenSettings {
        identity: ServiceIdentity {
            token_url: "https://sts.example.org/token".to_owned(),
            client_id: "front-door".to_owned(),
            client_secret_env: "KQ_SECRET".to_owned(),
            scope: Some("knowledge".to_owned()),
        },
        client_secret: SecretString::new("s3cret".to_owned()),
    }
}

impl Fixture {
    /// The desktop: a Person-mode client signed in to `ISSUER`.
    pub async fn new() -> Fixture {
        Self::with_timings(quick_timings()).await
    }

    pub async fn with_timings(timings: Timings) -> Fixture {
        Self::with_token_source(timings, |tokens| tokens as Arc<dyn TokenSource>).await
    }

    /// As `with_timings`, with the client's `TokenSource` built around the fake.
    pub async fn with_token_source(
        timings: Timings,
        source: impl FnOnce(Arc<FakeTokens>) -> Arc<dyn TokenSource>,
    ) -> Fixture {
        log_buffer();
        let dir = TempDir::new();
        let journal = open_journal(&dir).await;
        let tokens = FakeTokens::new();
        let client = KnowledgeQueryClient::with_parts(
            KnowledgeClientDeps {
                journal: Arc::clone(&journal),
                identity: None,
                http: reqwest::Client::new(),
                mode: KnowledgeClientMode::Person {
                    sign_in: CompanySignIn {
                        issuer: ISSUER.to_owned(),
                        client_id: CLIENT_ID.to_owned(),
                    },
                },
            },
            Some(source(Arc::clone(&tokens))),
            timings,
        )
        .expect("a Person client");
        // The background tasks start only in the tests that look at them.
        client
            .inner
            .background_started
            .store(true, Ordering::SeqCst);
        Fixture {
            client,
            journal,
            tokens,
            ws: WorkspaceId::new(),
            _dir: dir,
        }
    }

    /// The agent profile: an Agent-mode client with these proxies and a service identity.
    pub async fn agent_mode(proxies: &[&str]) -> Fixture {
        log_buffer();
        let dir = TempDir::new();
        let journal = open_journal(&dir).await;
        let client = KnowledgeQueryClient::with_parts(
            KnowledgeClientDeps {
                journal: Arc::clone(&journal),
                identity: None,
                http: reqwest::Client::new(),
                mode: KnowledgeClientMode::Agent {
                    proxies: proxies.iter().map(|n| proxy(n)).collect(),
                    service_token: Some(service_settings()),
                },
            },
            None,
            quick_timings(),
        )
        .expect("an Agent client");
        Fixture {
            client,
            journal,
            tokens: FakeTokens::new(),
            ws: WorkspaceId::new(),
            _dir: dir,
        }
    }

    pub fn pool(&self) -> sqlx::SqlitePool {
        self.journal.datastore().sqlite().expect("SQLite").clone()
    }

    /// Starts a loopback server and maps `https://<host>` to it.
    pub async fn stand_in(&self, host: &str, name: &str) -> Agent {
        let mock = MockServer::start().await;
        self.client
            .inner
            .transport
            .rewrites
            .lock()
            .unwrap()
            .insert(format!("https://{host}"), mock.uri());
        let _ = name;
        Agent { mock }
    }

    /// An agent that answers `about` and `contents` as a healthy front door.
    pub async fn agent(&self, host: &str, name: &str) -> Agent {
        let agent = self.stand_in(host, name).await;
        agent
            .about(about_json(name, ISSUER, &format!("aud-{name}")))
            .await;
        agent.contents(contents_json(name)).await;
        agent
    }

    pub fn ask(&self, target: AskTarget, query: &str) -> KnowledgeAsk {
        ask_of(target, query)
    }

    pub async fn journal_kinds(&self) -> Vec<String> {
        let page = self
            .journal
            .list_entries(ordin_journal::EntryFilter::default(), None)
            .await
            .expect("entries");
        page.items.into_iter().map(|e| e.kind).collect()
    }

    pub async fn event_kinds(&self) -> Vec<String> {
        if self.journal.last_seq() == 0 {
            return Vec::new();
        }
        self.journal
            .events_since(0, 500)
            .await
            .expect("events")
            .into_iter()
            .map(|e| e.kind)
            .collect()
    }
}

pub(crate) fn ask_of(target: AskTarget, query: &str) -> KnowledgeAsk {
    let now = tokio::time::Instant::now();
    KnowledgeAsk {
        target,
        query: Some(query.to_owned()),
        searches: None,
        limit: 20,
        access_group: None,
        hop_budget: 1,
        visited: Vec::new(),
        first_deadline: now + Duration::from_millis(1_000),
        more_until: now + Duration::from_millis(5_000),
        request_id: uuid::Uuid::now_v7(),
        asker: None,
    }
}

// ---- answers

pub(crate) fn about_json(name: &str, issuer: &str, audience: &str) -> Value {
    json!({
        "agent": name,
        "issuer": issuer,
        "token_audience": audience,
        "sources_record": { "enabled": true, "retention_days": 365 },
        "repository_count": 3,
        "as_of": null
    })
}

pub(crate) fn contents_json(name: &str) -> Value {
    json!({
        "agent_name": name,
        "generated_at": "2026-01-10T09:00:00.000Z",
        "repositories": [{
            "repository": "git.example.org/supply/handbook@main",
            "role": "reader",
            "last_commit": COMMIT,
            "last_fetched_at": "2026-01-10T08:00:00.000Z",
            "readable": true,
            "fact_count": 12
        }],
        "things": [
            { "name": "Borealis Ltd", "aliases": ["Borealis"], "link_count": 4 },
            { "name": "Leeds site", "aliases": [], "link_count": 2 }
        ]
    })
}

pub(crate) fn ts() -> Timestamp {
    Timestamp(parse_rfc3339_ms_utc("2026-01-10T08:00:00.000Z").unwrap())
}

pub(crate) fn hit(agent: &str, path: &str, statement: &str) -> KnowledgeHit {
    KnowledgeHit {
        statement: statement.to_owned(),
        agent: agent.to_owned(),
        repository: "git.example.org/supply/handbook@main".to_owned(),
        path: path.to_owned(),
        commit: COMMIT.to_owned(),
        section: None,
        page_title: None,
        kind: KnowledgeHitKind::Page,
        tier: Some(PageTier::Unverified),
        status: PageStatus::Stable,
        may_be_out_of_date: false,
        as_of: ts(),
        answer_id: None,
        shared_identity: None,
        things: Vec::new(),
        person_checked: false,
    }
}

pub(crate) fn result_event(hits: &[KnowledgeHit]) -> Value {
    json!({
        "request_id": uuid::Uuid::now_v7(),
        "hits": hits,
        "unanswered": [],
        "from_cache": false,
        "more_expected": false
    })
}

/// A server-sent-event body.
pub(crate) fn sse(events: &[(&str, Value)]) -> String {
    events
        .iter()
        .map(|(name, data)| format!("event: {name}\ndata: {data}\n\n"))
        .collect()
}

pub(crate) fn sse_template(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.into_bytes(), "text/event-stream")
}

impl Agent {
    pub async fn about(&self, body: Value) {
        Mock::given(method("GET"))
            .and(path("/v1/knowledge/about"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.mock)
            .await;
    }

    pub async fn contents(&self, body: Value) {
        Mock::given(method("GET"))
            .and(path("/v1/knowledge/contents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.mock)
            .await;
    }

    pub async fn route(&self, verb: &str, route: &str, template: impl wiremock::Respond + 'static) {
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(template)
            .mount(&self.mock)
            .await;
    }

    pub async fn query(&self, template: ResponseTemplate) {
        self.route("POST", "/v1/knowledge/query", template).await;
    }

    /// The agent answers one `result` with these hits.
    pub async fn answers(&self, hits: &[KnowledgeHit]) {
        self.query(sse_template(sse(&[("result", result_event(hits))])))
            .await;
    }

    /// How many requests of this method and path the agent has received.
    pub async fn count(&self, verb: &str, route: &str) -> usize {
        self.requests()
            .await
            .iter()
            .filter(|r| r.method.as_str() == verb && r.url.path() == route)
            .count()
    }

    pub async fn requests(&self) -> Vec<Request> {
        self.mock.received_requests().await.unwrap_or_default()
    }

    pub async fn query_requests(&self) -> Vec<Request> {
        self.requests()
            .await
            .into_iter()
            .filter(|r| r.url.path() == "/v1/knowledge/query")
            .collect()
    }
}

pub(crate) fn body_of(request: &Request) -> Value {
    serde_json::from_slice(&request.body).expect("a JSON body")
}

pub(crate) fn header_of(request: &Request, name: &str) -> String {
    request
        .headers
        .get(name)
        .map(|v| v.to_str().unwrap_or_default().to_owned())
        .unwrap_or_default()
}

/// Waits (up to 5 s) until `check` holds; fails the test otherwise.
pub(crate) async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let until = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < until {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

/// The log lines of the whole test binary. One global subscriber is installed before the first
/// fixture, so no call site is cached as "nobody listens" (plan FU-017).
static LOG: std::sync::OnceLock<Arc<Mutex<Vec<u8>>>> = std::sync::OnceLock::new();

#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn log_buffer() -> Arc<Mutex<Vec<u8>>> {
    Arc::clone(LOG.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let sink = Sink(Arc::clone(&buffer));
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || sink.clone())
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        buffer
    }))
}

/// Everything logged since `start`, by every test running in this binary.
pub(crate) struct LogCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
    from: usize,
}

impl LogCapture {
    pub fn start() -> LogCapture {
        let buffer = log_buffer();
        let from = buffer.lock().unwrap().len();
        LogCapture { buffer, from }
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.buffer.lock().unwrap()[self.from..]).into_owned()
    }
}
