//! `KnowledgeQueryClient`: the connections of a workspace (Internal Logic B to F), the shared state
//! the asking, linking and change-stream code runs on, and the program-facing operations.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_journal::{Journal, NewJournalEntry};
use ordin_types::common::SetBy;
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::{is_check_scope, KnowledgeAbout, KnowledgeContents};
use ordin_types::{KnowledgeConnectionId, WorkspaceId};
use serde_json::json;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::error::KqError;
use crate::store::{self, ConnRow};
use crate::tokens::{IdentityTokens, TokenSource};
use crate::transport::{read_plain, ServiceTokens, Transport};
use crate::types::*;

pub(crate) const COMPONENT: &str = "knowledge_query_client";
/// Contents above this size are stored with `things` emptied (Internal Logic B.8).
const MAX_CONTENTS_BYTES: usize = 2 * 1024 * 1024;
/// At most this many names leave `contents_names` (Internal Logic F).
const MAX_CONTENT_NAMES: usize = 4_000;

/// The durations the background code waits; the defaults are the 2D-4 constants.
#[derive(Debug, Clone)]
pub(crate) struct Timings {
    pub hedge_delay: Duration,
    pub backoff: Vec<Duration>,
    pub refresh_interval: Duration,
    pub about_timeout: Duration,
    pub network_allowance: Duration,
}

impl Default for Timings {
    fn default() -> Timings {
        Timings {
            hedge_delay: Duration::from_millis(HEDGE_DELAY_MS),
            backoff: CHANGE_STREAM_BACKOFF_SECONDS
                .iter()
                .map(|s| Duration::from_secs(*s))
                .collect(),
            refresh_interval: Duration::from_secs(CONTENTS_REFRESH_MINUTES * 60),
            about_timeout: Duration::from_secs(ABOUT_TIMEOUT_SECONDS),
            network_allowance: Duration::from_millis(NETWORK_ALLOWANCE_MS),
        }
    }
}

pub(crate) enum Mode {
    Person {
        sign_in: CompanySignIn,
    },
    Agent {
        proxies: Vec<ordin_types::domain::agent_profile::KnowledgeProxy>,
        service: Option<ServiceTokens>,
    },
}

/// Everything the tasks of the client share.
pub(crate) struct Inner {
    pub journal: Arc<Journal>,
    pub tokens: Option<Arc<dyn TokenSource>>,
    pub mode: Mode,
    pub transport: Transport,
    pub timings: Timings,
    /// ADR-115 fields of each connection's last-read `about` (SPEC-GAP-2: the Data Model has no
    /// column for them, so they live in memory and are re-read at start and at each refresh).
    pub about_cache: Mutex<HashMap<KnowledgeConnectionId, KnowledgeAbout>>,
    pub proxy_contents: Mutex<HashMap<String, KnowledgeContents>>,
    pub shutdown: CancellationToken,
    pub tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    pub background_started: AtomicBool,
}

pub struct KnowledgeQueryClient {
    pub(crate) inner: Arc<Inner>,
}

/// What reading a front door yields (steps B.4 to B.7).
pub(crate) struct AgentRead {
    pub about: KnowledgeAbout,
    pub contents_json: String,
}

pub(crate) fn now_hhmm() -> String {
    chrono::Utc::now().format("%H:%M").to_string()
}

/// One plain sentence for a failed read (`last_problem`, at most 300 characters).
pub(crate) fn problem_sentence(error: &KqError) -> String {
    match error {
        KqError::SignInNeeded => {
            "Sign in to your company account again to use this knowledge agent.".to_owned()
        }
        KqError::NotInAudience => "This knowledge agent serves another group of people.".to_owned(),
        KqError::OtherCompany => {
            "This knowledge agent accepts another company's sign-in.".to_owned()
        }
        _ => format!(
            "The knowledge agent could not be reached at {}.",
            now_hhmm()
        ),
    }
}

fn state_for(error: &KqError) -> ConnectionState {
    match error {
        KqError::SignInNeeded => ConnectionState::SignInNeeded,
        KqError::NotInAudience => ConnectionState::NotInAudience,
        _ => ConnectionState::Unreachable,
    }
}

/// The suffix of the sentence a refused account link leaves in `last_problem` (PD-A.3).
pub(crate) const LINK_PROBLEM_SUFFIX: &str =
    " did not accept ordin's sign-in for this knowledge agent.";

pub(crate) fn is_link_problem(problem: &str) -> bool {
    problem.ends_with(LINK_PROBLEM_SUFFIX)
}

/// Step B.1: the address must be https, without user information or fragment, at most 2,048
/// characters; the host is lower-cased and a trailing `/` removed.
pub(crate) fn normalise_url(raw: &str) -> Result<String, KqError> {
    if raw.chars().count() > 2048 || raw.chars().count() < 9 {
        return Err(KqError::UrlInvalid);
    }
    let parsed = url::Url::parse(raw).map_err(|_| KqError::UrlInvalid)?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || parsed.host_str().is_none()
    {
        return Err(KqError::UrlInvalid);
    }
    let text = parsed.as_str().trim_end_matches('/').to_owned();
    Ok(text)
}

fn view_of(row: &ConnRow) -> KnowledgeConnectionView {
    KnowledgeConnectionView {
        connection_id: row.id,
        name: row.name.clone(),
        url: row.url.clone(),
        added_by: row.added_by,
        sources_record: row.sources_record,
        last_answered_at: row.last_answered_at,
    }
}

impl KnowledgeQueryClient {
    /// Builds the client; opens no connection and logs nothing. `identity` must be Some in `Person`
    /// mode, and an `Agent` with proxies must hold a service token (else `KQ_CONFIG_INVALID`).
    pub fn new(deps: KnowledgeClientDeps) -> Result<Self, KqError> {
        let tokens: Option<Arc<dyn TokenSource>> = deps
            .identity
            .clone()
            .map(|identity| Arc::new(IdentityTokens(identity)) as Arc<dyn TokenSource>);
        Self::with_parts(deps, tokens, Timings::default())
    }

    pub(crate) fn with_parts(
        deps: KnowledgeClientDeps,
        tokens: Option<Arc<dyn TokenSource>>,
        timings: Timings,
    ) -> Result<Self, KqError> {
        let mode = match deps.mode {
            KnowledgeClientMode::Person { sign_in } => {
                if tokens.is_none() {
                    return Err(KqError::ConfigInvalid);
                }
                Mode::Person { sign_in }
            }
            KnowledgeClientMode::Agent {
                proxies,
                service_token,
            } => {
                if !proxies.is_empty() && service_token.is_none() {
                    return Err(KqError::ConfigInvalid);
                }
                Mode::Agent {
                    proxies,
                    service: service_token.map(ServiceTokens::new),
                }
            }
        };
        Ok(KnowledgeQueryClient {
            inner: Arc::new(Inner {
                journal: deps.journal,
                tokens,
                mode,
                transport: Transport::new(deps.http),
                timings,
                about_cache: Mutex::new(HashMap::new()),
                proxy_contents: Mutex::new(HashMap::new()),
                shutdown: CancellationToken::new(),
                tasks: Mutex::new(Vec::new()),
                background_started: AtomicBool::new(false),
            }),
        })
    }

    /// Closes every open connection and change stream (graceful shutdown). Asks in flight end with
    /// their sources listed `Unreachable`.
    pub async fn close(&self) {
        self.inner.shutdown.cancel();
        let handles: Vec<_> = match self.inner.tasks.lock() {
            Ok(mut tasks) => tasks.drain(..).collect(),
            Err(_) => Vec::new(),
        };
        for handle in handles {
            handle.abort();
        }
        self.inner.transport.close();
    }

    // ---- desktop and personal server: a workspace's front doors

    pub async fn list_connections(
        &self,
        ws: WorkspaceId,
    ) -> Result<Vec<KnowledgeConnectionView>, KqError> {
        self.inner.require_person()?;
        self.inner.ensure_background();
        let pool = self.inner.pool()?;
        Ok(store::live_of(&pool, ws)
            .await?
            .iter()
            .map(view_of)
            .collect())
    }

    pub async fn add_connection(
        &self,
        ws: WorkspaceId,
        url: &str,
    ) -> Result<(KnowledgeConnectionView, KnowledgeAbout), KqError> {
        let inner = &self.inner;
        let url = normalise_url(url)?;
        let sign_in = inner.person_sign_in()?.clone();
        inner.ensure_background();
        let pool = inner.pool()?;
        if store::live_of(&pool, ws).await?.len() >= MAX_CONNECTIONS_PER_WORKSPACE {
            return Err(KqError::TooManyConnections);
        }
        if store::url_taken(&pool, ws, &url).await? {
            return Err(KqError::AlreadyConnected);
        }
        let read = inner.read_agent(&url, &sign_in).await?;
        let row = ConnRow {
            id: KnowledgeConnectionId::new(),
            workspace_id: ws,
            url,
            name: read.about.agent.clone(),
            issuer: read.about.issuer.clone(),
            token_audience: read.about.token_audience.clone(),
            added_by: SetBy::Person,
            company_entry_key: None,
            removed_by_person: false,
            state: ConnectionState::Ready,
            sources_record: read.about.sources_record,
            last_problem: None,
            added_at: Timestamp::now(),
            last_answered_at: None,
        };
        store::insert(&pool, &row, Some(&read.contents_json)).await?;
        if let Ok(mut cache) = inner.about_cache.lock() {
            cache.insert(row.id, read.about.clone());
        }
        inner
            .journal_change(
                "knowledge_agent_connected",
                "A knowledge agent was connected to this workspace.",
                &row,
            )
            .await;
        inner.event_changed(ws).await;
        inner.link_connection(&row, None).await;
        Ok((view_of(&row), read.about))
    }

    pub async fn remove_connection(&self, id: KnowledgeConnectionId) -> Result<(), KqError> {
        let inner = &self.inner;
        inner.require_person()?;
        let pool = inner.pool()?;
        let row = match store::get(&pool, id).await? {
            Some(row) if !row.removed_by_person => row,
            _ => return Err(KqError::ConnectionNotFound),
        };
        match row.added_by {
            SetBy::Person => store::delete(&pool, id).await?,
            SetBy::Company => store::mark_removed(&pool, id).await?,
        }
        if let Ok(mut cache) = inner.about_cache.lock() {
            cache.remove(&id);
        }
        inner
            .journal_change(
                "knowledge_agent_disconnected",
                "A knowledge agent was disconnected from this workspace.",
                &row,
            )
            .await;
        inner.event_changed(row.workspace_id).await;
        Ok(())
    }

    pub async fn details(
        &self,
        id: KnowledgeConnectionId,
    ) -> Result<KnowledgeConnectionDetails, KqError> {
        let inner = &self.inner;
        let sign_in = inner.person_sign_in()?.clone();
        inner.ensure_background();
        let pool = inner.pool()?;
        let row = match store::get(&pool, id).await? {
            Some(row) if !row.removed_by_person => row,
            _ => return Err(KqError::ConnectionNotFound),
        };
        let mut about = None;
        match inner.read_agent(&row.url, &sign_in).await {
            Ok(read) => {
                inner.apply_read(&pool, &row, &read).await?;
                about = Some(read.about);
            }
            Err(error) => {
                let problem = problem_sentence(&error);
                store::set_problem(&pool, id, Some(&problem)).await?;
                tracing::debug!(
                    component = COMPONENT,
                    connection_id = id.to_string().as_str(),
                    code = error.code(),
                    "kq_details_refresh_failed"
                );
            }
        }
        let row = store::get(&pool, id)
            .await?
            .ok_or(KqError::ConnectionNotFound)?;
        let (repositories, as_of) = match store::contents_of(&pool, id).await? {
            Some((contents, _)) => {
                let oldest = contents
                    .repositories
                    .iter()
                    .filter_map(|r| r.last_fetched_at)
                    .min_by_key(|t| t.0);
                (contents.repositories, oldest)
            }
            None => (Vec::new(), None),
        };
        let accounts = inner.accounts_view(&pool, &row).await?;
        Ok(KnowledgeConnectionDetails {
            connection: view_of(&row),
            about,
            state: row.state,
            repositories,
            as_of,
            last_problem: row.last_problem.clone(),
            accounts,
        })
    }

    /// Reconciles company-named front doors (Internal Logic D).
    pub async fn apply_company_front_doors(
        &self,
        per_workspace: Vec<(WorkspaceId, Vec<CompanyFrontDoor>)>,
    ) -> Result<(), KqError> {
        let inner = &self.inner;
        let sign_in = inner.person_sign_in()?.clone();
        inner.ensure_background();
        let pool = inner.pool()?;
        for (ws, entries) in per_workspace {
            let existing = store::company_rows(&pool, ws).await?;
            let mut changed = false;
            let keys: HashSet<&str> = entries.iter().map(|e| e.key.as_str()).collect();
            for row in &existing {
                let key = row.company_entry_key.as_deref().unwrap_or_default();
                if !keys.contains(key) {
                    store::delete(&pool, row.id).await?;
                    inner
                        .journal_change(
                            "knowledge_agent_disconnected",
                            "A knowledge agent was disconnected from this workspace.",
                            row,
                        )
                        .await;
                    changed = true;
                }
            }
            for entry in &entries {
                let known = existing
                    .iter()
                    .any(|r| r.company_entry_key.as_deref() == Some(entry.key.as_str()));
                if known {
                    continue;
                }
                let Ok(url) = normalise_url(&entry.url) else {
                    tracing::warn!(
                        component = COMPONENT,
                        entry_key = entry.key.as_str(),
                        "kq_company_front_door_invalid"
                    );
                    continue;
                };
                // SPEC-GAP-6: the row needs an issuer and an audience before `about` is read; the
                // person's sign-in issuer and the front door's address stand in until the
                // background read replaces them.
                let row = ConnRow {
                    id: KnowledgeConnectionId::new(),
                    workspace_id: ws,
                    url: url.clone(),
                    name: entry.display_name.chars().take(80).collect(),
                    issuer: sign_in.issuer.clone(),
                    token_audience: url.clone(),
                    added_by: SetBy::Company,
                    company_entry_key: Some(entry.key.clone()),
                    removed_by_person: false,
                    state: ConnectionState::Unreachable,
                    sources_record: None,
                    last_problem: None,
                    added_at: Timestamp::now(),
                    last_answered_at: None,
                };
                if store::insert(&pool, &row, None).await.is_err() {
                    // The same address is already connected to this workspace.
                    continue;
                }
                inner
                    .journal_change(
                        "knowledge_agent_connected",
                        "A knowledge agent was connected to this workspace.",
                        &row,
                    )
                    .await;
                changed = true;
                inner.spawn_refresh(row);
            }
            if changed {
                inner.event_changed(ws).await;
            }
        }
        Ok(())
    }

    /// The live connections of a workspace (not removed by the person); empty on A. Never fails.
    pub async fn connections_of(&self, ws: WorkspaceId) -> Vec<KnowledgeConnection> {
        if !matches!(self.inner.mode, Mode::Person { .. }) {
            return Vec::new();
        }
        self.inner.ensure_background();
        let rows = match self.inner.pool() {
            Ok(pool) => store::live_of(&pool, ws).await,
            Err(error) => Err(error),
        };
        match rows {
            Ok(rows) => rows
                .into_iter()
                .map(|r| KnowledgeConnection {
                    id: r.id,
                    name: r.name,
                    url: r.url,
                    added_by: r.added_by,
                    state: r.state,
                })
                .collect(),
            Err(_) => {
                tracing::error!(component = COMPONENT, "kq_store_failed");
                Vec::new()
            }
        }
    }

    /// Entity names and aliases of the workspace's front doors' cached tables of contents; no
    /// network call.
    pub async fn contents_names(&self, ws: WorkspaceId) -> Vec<String> {
        let Ok(pool) = self.inner.pool() else {
            return Vec::new();
        };
        let all = match store::live_contents(&pool, ws).await {
            Ok(all) => all,
            Err(_) => {
                tracing::error!(component = COMPONENT, "kq_store_failed");
                return Vec::new();
            }
        };
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        'outer: for contents in all {
            for thing in contents.things {
                for name in std::iter::once(thing.name).chain(thing.aliases) {
                    if seen.insert(name.clone()) {
                        names.push(name);
                        if names.len() >= MAX_CONTENT_NAMES {
                            break 'outer;
                        }
                    }
                }
            }
        }
        names
    }

    /// ADR-115: the `sharepoint_check_scope` of every live connection's stored `about`,
    /// de-duplicated; empty on A. No network call.
    pub async fn microsoft_check_scopes(&self) -> Vec<String> {
        if !matches!(self.inner.mode, Mode::Person { .. }) {
            return Vec::new();
        }
        let Ok(pool) = self.inner.pool() else {
            return Vec::new();
        };
        let Ok(rows) = store::all_live(&pool).await else {
            return Vec::new();
        };
        let Ok(cache) = self.inner.about_cache.lock() else {
            return Vec::new();
        };
        let mut scopes: Vec<String> = Vec::new();
        for row in rows {
            if let Some(scope) = cache
                .get(&row.id)
                .and_then(|e| e.sharepoint_check_scope.clone())
            {
                if !scopes.contains(&scope) {
                    scopes.push(scope);
                }
            }
        }
        scopes
    }
}

fn trimmed_json(contents: &KnowledgeContents) -> String {
    let mut cut = contents.clone();
    cut.things.clear();
    tracing::warn!(component = COMPONENT, "kq_contents_trimmed");
    serde_json::to_string(&cut).unwrap_or_default()
}

impl Inner {
    pub(crate) fn pool(&self) -> Result<SqlitePool, KqError> {
        self.journal
            .datastore()
            .sqlite()
            .cloned()
            .ok_or(KqError::StoreUnavailable)
    }

    pub(crate) fn require_person(&self) -> Result<(), KqError> {
        self.person_sign_in().map(|_| ())
    }

    pub(crate) fn person_sign_in(&self) -> Result<&CompanySignIn, KqError> {
        match &self.mode {
            Mode::Person { sign_in } => Ok(sign_in),
            Mode::Agent { .. } => Err(KqError::NotSupported),
        }
    }

    pub(crate) fn token_source(&self) -> Result<&Arc<dyn TokenSource>, KqError> {
        self.tokens.as_ref().ok_or(KqError::ConfigInvalid)
    }

    /// The person's company token for one audience; a permanent sign-in code is
    /// `KQ_SIGN_IN_NEEDED`, a retryable one `KQ_UNREACHABLE` (B.6).
    pub(crate) async fn person_token(
        &self,
        sign_in: &CompanySignIn,
        audience: &str,
    ) -> Result<String, KqError> {
        let token = self
            .token_source()?
            .audience_token(&sign_in.issuer, &sign_in.client_id, audience)
            .await
            .map_err(|error| match error {
                ordin_identity::CredError::Retryable { .. } => KqError::Unreachable,
                ordin_identity::CredError::Permanent { .. } => KqError::SignInNeeded,
            })?;
        Ok(token.token.expose().to_owned())
    }

    /// Steps B.4 to B.7: `about`, the issuer check, the token and the contents.
    pub(crate) async fn read_agent(
        &self,
        url: &str,
        sign_in: &CompanySignIn,
    ) -> Result<AgentRead, KqError> {
        let about_request = self
            .transport
            .client(url)
            .get(self.transport.address(format!("{url}/v1/knowledge/about")))
            .timeout(self.timings.about_timeout);
        let plain = read_plain(about_request)
            .await
            .map_err(|_| KqError::Unreachable)?;
        if plain.status != 200 {
            return Err(KqError::NotAKnowledgeAgent);
        }
        let mut value: serde_json::Value =
            serde_json::from_slice(&plain.body).map_err(|_| KqError::NotAKnowledgeAgent)?;
        // An entry naming a system this crate does not know is skipped, so a newer front door reads.
        if let Some(accounts) = value.get_mut("accounts").and_then(|v| v.as_array_mut()) {
            accounts.retain(|entry| serde_json::from_value::<AccountNeed>(entry.clone()).is_ok());
        }
        let about: KnowledgeAbout =
            serde_json::from_value(value).map_err(|_| KqError::NotAKnowledgeAgent)?;
        if about.agent.is_empty()
            || about.agent.chars().count() > 80
            || about.issuer.chars().count() < 9
            || about.token_audience.is_empty()
            || about.token_audience.chars().count() > 512
            || about
                .sharepoint_check_scope
                .as_deref()
                .is_some_and(|scope| !is_check_scope(scope))
        {
            return Err(KqError::NotAKnowledgeAgent);
        }
        // spec: ordin-desktop/components/KnowledgeQueryClient#SA-KQC-02
        if about.issuer.trim_end_matches('/') != sign_in.issuer.trim_end_matches('/') {
            return Err(KqError::OtherCompany);
        }
        let contents = self
            .fetch_contents(url, sign_in, &about.token_audience)
            .await?;
        let mut contents_json =
            serde_json::to_string(&contents).map_err(|_| KqError::Unreachable)?;
        if contents_json.len() > MAX_CONTENTS_BYTES {
            contents_json = trimmed_json(&contents);
        }
        Ok(AgentRead {
            about,
            contents_json,
        })
    }

    async fn fetch_contents(
        &self,
        url: &str,
        sign_in: &CompanySignIn,
        audience: &str,
    ) -> Result<KnowledgeContents, KqError> {
        let mut token = self.person_token(sign_in, audience).await?;
        let mut retried = false;
        loop {
            let request = self
                .transport
                .client(url)
                .get(
                    self.transport
                        .address(format!("{url}/v1/knowledge/contents")),
                )
                .bearer_auth(&token)
                .timeout(self.timings.about_timeout);
            let plain = read_plain(request)
                .await
                .map_err(|_| KqError::Unreachable)?;
            match plain.status {
                200 => {
                    return serde_json::from_slice(&plain.body).map_err(|_| KqError::Unreachable)
                }
                401 if !retried => {
                    retried = true;
                    self.token_source()?
                        .invalidate_audience(&sign_in.issuer, &sign_in.client_id, audience)
                        .await;
                    token = self.person_token(sign_in, audience).await?;
                }
                401 => return Err(KqError::SignInNeeded),
                403 if code_of(&plain.body) == Some("KQ_NOT_IN_AUDIENCE") => {
                    return Err(KqError::NotInAudience)
                }
                _ => return Err(KqError::Unreachable),
            }
        }
    }

    /// Stores what a successful read found: the front door's own description, its contents, a
    /// `ready` state, and the in-memory ADR-115 fields.
    pub(crate) async fn apply_read(
        &self,
        pool: &SqlitePool,
        row: &ConnRow,
        read: &AgentRead,
    ) -> Result<(), KqError> {
        store::set_about(
            pool,
            row.id,
            &read.about.issuer,
            &read.about.token_audience,
            read.about.sources_record,
        )
        .await?;
        store::upsert_contents(pool, row.id, &read.contents_json).await?;
        // A sentence about a refused account link stays until a link succeeds (PD-A.3).
        let keep = row.last_problem.as_deref().filter(|p| is_link_problem(p));
        store::set_state(pool, row.id, ConnectionState::Ready, keep).await?;
        if let Ok(mut cache) = self.about_cache.lock() {
            cache.insert(row.id, read.about.clone());
        }
        Ok(())
    }

    /// One refresh of a connection (F): read, store, or record why it failed. Returns whether it
    /// succeeded.
    pub(crate) async fn refresh_connection(&self, row: &ConnRow) -> bool {
        let Ok(sign_in) = self.person_sign_in().cloned() else {
            return false;
        };
        let Ok(pool) = self.pool() else {
            return false;
        };
        match self.read_agent(&row.url, &sign_in).await {
            Ok(read) => self.apply_read(&pool, row, &read).await.is_ok(),
            Err(error) => {
                let problem = problem_sentence(&error);
                let _ = store::set_state(&pool, row.id, state_for(&error), Some(&problem)).await;
                tracing::debug!(
                    component = COMPONENT,
                    connection_id = row.id.to_string().as_str(),
                    code = error.code(),
                    "kq_contents_refresh_failed"
                );
                false
            }
        }
    }

    /// Steps B.4 to B.7 for a new company row, then linking, off the caller's path (Internal Logic D).
    pub(crate) fn spawn_refresh(self: &Arc<Self>, row: ConnRow) {
        let inner = Arc::clone(self);
        let handle = tokio::spawn(async move {
            if inner.refresh_connection(&row).await {
                inner.link_connection(&row, None).await;
            }
        });
        self.track(handle);
    }

    pub(crate) fn track(&self, handle: tokio::task::JoinHandle<()>) {
        if let Ok(mut tasks) = self.tasks.lock() {
            tasks.retain(|t| !t.is_finished());
            tasks.push(handle);
        }
    }

    /// Starts, once, the start-up pass, the hourly refresh and the sign-in listener (Person mode).
    /// SPEC-GAP-4: the spec names no start method for these; the first call of a program-facing
    /// operation starts them.
    pub(crate) fn ensure_background(self: &Arc<Self>) {
        if !matches!(self.mode, Mode::Person { .. }) {
            return;
        }
        if self.background_started.swap(true, Ordering::SeqCst) {
            return;
        }
        self.start_maintenance();
        self.start_sign_in_loop();
    }

    pub(crate) fn start_maintenance(self: &Arc<Self>) {
        let inner = Arc::clone(self);
        self.track(tokio::spawn(async move { inner.maintenance_loop().await }));
    }

    pub(crate) fn start_sign_in_loop(self: &Arc<Self>) {
        let inner = Arc::clone(self);
        self.track(tokio::spawn(async move { inner.sign_in_loop().await }));
    }

    async fn maintenance_loop(self: Arc<Self>) {
        let mut first = true;
        loop {
            let rows = match self.pool() {
                Ok(pool) => store::all_live(&pool).await.unwrap_or_default(),
                Err(_) => Vec::new(),
            };
            for row in rows {
                let ok = self.refresh_connection(&row).await;
                if ok && first {
                    // PD-A.1 (b): linking at start, after the contents refresh.
                    self.link_connection(&row, None).await;
                }
            }
            first = false;
            tokio::select! {
                _ = tokio::time::sleep(self.timings.refresh_interval) => {}
                _ = self.shutdown.cancelled() => return,
            }
        }
    }

    pub(crate) async fn journal_change(&self, kind: &str, summary: &str, row: &ConnRow) {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: COMPONENT.to_owned(),
            run_id: None,
            crew_run_id: None,
            workspace_id: Some(row.workspace_id),
            summary: summary.to_owned(),
            data: json!({
                "connection_id": row.id,
                "added_by": store::added_by_text(row.added_by),
            }),
        };
        if self.journal.append_entry(entry).await.is_err() {
            tracing::error!(component = COMPONENT, "kq_store_failed");
        }
    }

    pub(crate) async fn event_changed(&self, ws: WorkspaceId) {
        if self
            .journal
            .append_event("knowledge_agents_changed", json!({ "workspace_id": ws }))
            .await
            .is_err()
        {
            tracing::error!(component = COMPONENT, "kq_store_failed");
        }
    }
}

/// The `code` of a Service API error body, when it has one.
pub(crate) fn code_of(body: &[u8]) -> Option<&'static str> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    match value.get("code")?.as_str()? {
        "KQ_NOT_IN_AUDIENCE" => Some("KQ_NOT_IN_AUDIENCE"),
        "KQ_LOOP" => Some("KQ_LOOP"),
        _ => None,
    }
}
