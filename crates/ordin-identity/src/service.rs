//! `IdentityService`: start-up (Internal Logic A), the credential map, persistence of a sign-in (C.10,
//! C.11), tokens and refresh (E, H.1–H.3), and the state readers (G, H.4, H.6).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use ordin_types::domain::identity::{Credential, CredentialKind, CredentialState};
use ordin_types::time::Timestamp;
use ordin_types::CredentialId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use zeroize::{Zeroize, Zeroizing};

use crate::config::{check_config, BrowserOpener, IdentityConfig, Program, SecretBackendConfig};
use crate::error::{code, fail, CredError};
use crate::oauth::{self, Http, Outcome, HTTP_TIMEOUT};
use crate::secret::{
    Backend, KeyringKeychain, SecretBackendKind, SecretName, SecretStore, SecretString,
};
use crate::secret_file::EncryptedFile;
use crate::sign_in::Pending;
use crate::store::{self, kind_str, Row};
use crate::types::{
    AccessToken, CredentialView, PersonProfile, RefreshReport, SignInChallengeId, SignInChanged,
    SignInState,
};

/// Refresh an access token when it expires within this margin (Fixed constants; SA-RECON-06).
pub(crate) const REFRESH_MARGIN_MS: i64 = 5 * 60 * 1000;
/// Capacity of the `SignInChanged` channel.
const EVENT_CAPACITY: usize = 64;

/// The token blob kept in the secret store under `credential/<kind>/<id>` (C.10).
///
/// SPEC-GAP-3: C.10 lists `access_token`, `refresh_token`, `expires_at`, `scopes`, `token_type`, and §J
/// adds `method`, `email`, `account_id`. A refresh (E.4) needs the token endpoint and the client id, and
/// Google's refresh needs the Desktop client's non-confidential secret; H.2 says the client id is in the
/// blob. The blob therefore also carries `client_id`, `token_endpoint` and, for Google only,
/// `client_secret`. None of them is a confidential secret, and all stay in the secret store.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TokenBlob {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<Timestamp>,
    pub scopes: Vec<String>,
    pub token_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

impl Drop for TokenBlob {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(refresh) = self.refresh_token.as_mut() {
            refresh.zeroize();
        }
    }
}

impl TokenBlob {
    fn to_bytes(&self) -> Result<Zeroizing<Vec<u8>>, CredError> {
        serde_json::to_vec(self)
            .map(Zeroizing::new)
            .or_else(|_| fail(code::STORE_FAILED))
    }
}

/// A token cached for one audience (H.2).
struct AudienceToken {
    token: Zeroizing<String>,
    expires_at: Option<Timestamp>,
    credential_id: CredentialId,
}

pub(crate) struct Inner {
    pub program: Program,
    pub github_app_client_id: Option<String>,
    pub browser: Option<Arc<dyn BrowserOpener>>,
    pub http: Http,
    pub pool: Option<SqlitePool>,
    pub secrets: Arc<SecretStore>,
    /// Every active credential with its meta row (A.3), keyed by id.
    pub rows: RwLock<HashMap<CredentialId, Row>>,
    /// Token blobs read once and kept until sign-out or refresh (E.3).
    tokens: Mutex<HashMap<CredentialId, Arc<TokenBlob>>>,
    /// One refresh per credential at a time (E.4).
    refresh_locks: Mutex<HashMap<CredentialId, Arc<tokio::sync::Mutex<()>>>>,
    audience: Mutex<HashMap<(String, String, String), AudienceToken>>,
    pub pending: Mutex<HashMap<SignInChallengeId, Pending>>,
    pub certificates: Mutex<HashMap<String, Arc<rustls::sign::CertifiedKey>>>,
    events: broadcast::Sender<SignInChanged>,
    /// How long a browser or relayed sign-in stays open (Fixed constants: 10 minutes).
    pub sign_in_wait: Duration,
}

/// The one service of this crate.
pub struct IdentityService {
    pub(crate) inner: Arc<Inner>,
}

/// Locks a std mutex, taking the data of a poisoned one (a panic elsewhere does not lose the map).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The first 8 hexadecimal characters of SHA-256, how labels and names are logged (Security).
pub(crate) fn short_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn is_password_kind(kind: CredentialKind) -> bool {
    matches!(
        kind,
        CredentialKind::MailServerPassword | CredentialKind::MailServerAppPassword
    )
}

pub(crate) fn is_mail_kind(kind: CredentialKind) -> bool {
    is_password_kind(kind) || kind == CredentialKind::MailServerSingleSignOn
}

fn expires_within_margin(expires_at: Option<Timestamp>) -> bool {
    expires_at
        .is_some_and(|at| at.unix_millis() - Timestamp::now().unix_millis() <= REFRESH_MARGIN_MS)
}

fn expired(expires_at: Option<Timestamp>) -> bool {
    expires_at.is_some_and(|at| at.unix_millis() <= Timestamp::now().unix_millis())
}

/// `credential/<kind>/<credential id>`.
pub(crate) fn credential_secret_name(kind: CredentialKind, id: CredentialId) -> String {
    format!("credential/{}/{id}", kind_str(kind))
}

impl IdentityService {
    /// `pool` is the SQLite pool of the Journal's `Datastore`, handed over by the program because this
    /// phase-1 crate has no Journal edge (D4, R20); None only for `AgentScaledShape`.
    // spec: ordin-desktop/components/IdentityAndCredentials#SA-RECON-01
    pub async fn new(config: IdentityConfig, pool: Option<SqlitePool>) -> Result<Self, CredError> {
        check_config(&config, pool.is_some())?;
        let backend = match &config.secret_backend {
            SecretBackendConfig::OsKeychain => Backend::Keychain {
                kind: SecretBackendKind::OsKeychain,
                keychain: Arc::new(KeyringKeychain {
                    service: config.keychain_service.clone(),
                }),
            },
            SecretBackendConfig::SecretService => Backend::Keychain {
                kind: SecretBackendKind::SecretService,
                keychain: Arc::new(KeyringKeychain {
                    service: config.keychain_service.clone(),
                }),
            },
            SecretBackendConfig::EncryptedFile { key_source } => {
                Backend::File(EncryptedFile::open(&config.data_root, key_source).await?)
            }
            SecretBackendConfig::None => Backend::None,
        };
        let http = Http {
            client: config.http.clone(),
            timeout: HTTP_TIMEOUT,
            #[cfg(test)]
            redirect: None,
        };
        Self::start(config, pool, SecretStore::from_backend(backend), http).await
    }

    /// A.2–A.4 with the store already open; the unit tests enter here with their own store and client.
    pub(crate) async fn start(
        config: IdentityConfig,
        pool: Option<SqlitePool>,
        secrets: SecretStore,
        http: Http,
    ) -> Result<Self, CredError> {
        tracing::info!(
            target: "ordin_identity",
            backend = secrets.backend().as_str(),
            "cred.secret_store.opened"
        );
        let rows = match &pool {
            Some(pool) => store::load_active(pool).await?,
            None => Vec::new(),
        };
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let service = IdentityService {
            inner: Arc::new(Inner {
                program: config.program,
                github_app_client_id: config.github_app_client_id,
                browser: config.browser,
                http,
                pool,
                secrets: Arc::new(secrets),
                rows: RwLock::new(rows.iter().map(|r| (r.credential.id, r.clone())).collect()),
                tokens: Mutex::new(HashMap::new()),
                refresh_locks: Mutex::new(HashMap::new()),
                audience: Mutex::new(HashMap::new()),
                pending: Mutex::new(HashMap::new()),
                certificates: Mutex::new(HashMap::new()),
                events,
                sign_in_wait: Duration::from_secs(10 * 60),
            }),
        };
        // A.4: an active row whose secret item is missing is marked; a store that cannot answer now
        // leaves the row as it is (the first use reports the store's error).
        for row in rows {
            let Ok(name) = SecretName::new(&row.credential.keychain_ref) else {
                continue;
            };
            if let Ok(None) = service.inner.secrets.get(&name).await {
                service
                    .inner
                    .mark_needs_sign_in(&row, code::KEYCHAIN_MISSING, None, false)
                    .await?;
                tracing::warn!(
                    target: "ordin_identity",
                    credential_id = row.credential.id.to_string().as_str(),
                    kind = kind_str(row.credential.kind),
                    "cred.secret_missing"
                );
            }
        }
        Ok(service)
    }

    /// A shared handle on the one secret store this service holds; each call returns a clone of the same `Arc`.
    pub fn secrets(&self) -> Arc<SecretStore> {
        Arc::clone(&self.inner.secrets)
    }

    /// Notified on every change of a credential's SignInState.
    pub fn subscribe(&self) -> broadcast::Receiver<SignInChanged> {
        self.inner.events.subscribe()
    }

    // ---- E: tokens

    /// A valid access token, refreshed first when it expires within 5 minutes.
    /// `account_label` None selects the single active credential of that kind (CRED_AMBIGUOUS if several).
    pub async fn access_token(
        &self,
        kind: CredentialKind,
        account_label: Option<&str>,
    ) -> Result<AccessToken, CredError> {
        let row = self.inner.select(kind, account_label)?;
        self.inner.token_of(&row).await
    }

    /// As `access_token(kind, None)`, and the credential's granted scopes must contain every entry of
    /// `scopes`; otherwise CRED_SCOPE_MISSING, before any provider request.
    pub async fn access_token_for(
        &self,
        kind: CredentialKind,
        scopes: &[&str],
    ) -> Result<AccessToken, CredError> {
        let row = self.inner.select(kind, None)?;
        if row.meta.needs_sign_in {
            return fail(code::SIGN_IN_REQUIRED);
        }
        if !scopes
            .iter()
            .all(|s| row.meta.scopes.iter().any(|g| g == s))
        {
            return fail(code::SCOPE_MISSING);
        }
        self.inner.token_of(&row).await
    }

    /// The token of one credential by id, for callers that stored the id.
    pub async fn access_token_by_id(
        &self,
        credential_id: CredentialId,
    ) -> Result<AccessToken, CredError> {
        let row = self.inner.row(credential_id)?;
        if is_password_kind(row.credential.kind) {
            return fail(code::KIND_HAS_NO_TOKEN);
        }
        self.inner.token_of(&row).await
    }

    /// Refresh with the stored refresh token only, never a prompt. `Ok(true)` when a new access token
    /// was obtained, `Ok(false)` when there is no refresh token or the provider refused it.
    pub async fn refresh_silently(&self, kind: CredentialKind) -> Result<bool, CredError> {
        if is_password_kind(kind) {
            return fail(code::KIND_HAS_NO_TOKEN);
        }
        let row = match self.inner.select(kind, None) {
            Ok(row) => row,
            Err(e) if ordin_types::CodedError::code(&e) == code::SIGN_IN_REQUIRED => {
                return Ok(false)
            }
            Err(e) => return Err(e),
        };
        let gate = self.inner.refresh_lock(row.credential.id);
        let _one = gate.lock().await;
        let blob = self.inner.read_blob(&row).await?;
        if blob.refresh_token.is_none() {
            return Ok(false);
        }
        match self.inner.refresh(&row, &blob).await {
            Ok(_) => Ok(true),
            Err(e) if ordin_types::CodedError::code(&e) == code::SIGN_IN_REQUIRED => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The person's company (OIDC) access token for one audience (H.2).
    pub async fn access_token_for_audience(
        &self,
        issuer: &str,
        client_id: &str,
        audience: &str,
    ) -> Result<AccessToken, CredError> {
        let chars = audience.chars().count();
        if chars == 0 || chars > 512 {
            return fail(code::URL_INVALID);
        }
        let candidates: Vec<Row> = self
            .inner
            .active()
            .into_iter()
            .filter(|r| r.credential.kind == CredentialKind::Oidc && r.meta.provider_host == issuer)
            .collect();
        let mut chosen = None;
        for row in candidates {
            let blob = self.inner.read_blob(&row).await?;
            if blob.client_id.as_deref() == Some(client_id) {
                chosen = Some((row, blob));
                break;
            }
        }
        let Some((row, blob)) = chosen else {
            return fail(code::SIGN_IN_REQUIRED);
        };
        if row.meta.needs_sign_in {
            return fail(code::SIGN_IN_REQUIRED);
        }
        let key = (issuer.to_owned(), client_id.to_owned(), audience.to_owned());
        if let Some(cached) = lock(&self.inner.audience).get(&key) {
            if !expires_within_margin(cached.expires_at) {
                return Ok(AccessToken {
                    token: SecretString::new(cached.token.to_string()),
                    expires_at: cached.expires_at,
                    credential_id: cached.credential_id,
                });
            }
        }
        let gate = self.inner.refresh_lock(row.credential.id);
        let _one = gate.lock().await;
        let blob = Arc::clone(&blob);
        let (Some(refresh_token), Some(endpoint)) = (&blob.refresh_token, &blob.token_endpoint)
        else {
            return fail(code::SIGN_IN_REQUIRED);
        };
        let entra = url::Url::parse(issuer)
            .ok()
            .and_then(|u| u.host_str().map(|h| h == "login.microsoftonline.com"))
            .unwrap_or(false);
        let entra_scope = format!("{audience}/.default");
        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ];
        if entra {
            form.push(("scope", &entra_scope));
        } else {
            form.push(("audience", audience));
        }
        let reply = self
            .inner
            .http
            .post_form(endpoint, &form, None, self.inner.http.timeout)
            .await;
        let tokens = match reply.and_then(oauth::read_tokens) {
            Ok(Outcome::Tokens(tokens)) => tokens,
            Ok(Outcome::Refused { error, status }) => {
                return Err(self.inner.refresh_failed(&row, &error, status).await)
            }
            Err(e) => {
                self.inner
                    .log_refresh_failed(&row, ordin_types::CodedError::code(&e));
                return Err(e);
            }
        };
        if let Some(rotated) = &tokens.refresh_token {
            let mut next = (*blob).clone();
            next.refresh_token = Some(rotated.to_string());
            self.inner.write_blob(&row, next).await?;
        }
        let token = AccessToken {
            token: SecretString::new(tokens.access_token.to_string()),
            expires_at: tokens.expires_at,
            credential_id: row.credential.id,
        };
        lock(&self.inner.audience).insert(
            key,
            AudienceToken {
                token: tokens.access_token.clone(),
                expires_at: tokens.expires_at,
                credential_id: row.credential.id,
            },
        );
        Ok(token)
    }

    /// Drops the cached token of that audience, so the next call refreshes it.
    pub async fn invalidate_audience_token(&self, issuer: &str, client_id: &str, audience: &str) {
        lock(&self.inner.audience).remove(&(
            issuer.to_owned(),
            client_id.to_owned(),
            audience.to_owned(),
        ));
    }

    // ---- G: state readers

    pub async fn sign_in_state(&self, kind: CredentialKind) -> Result<SignInState, CredError> {
        let rows: Vec<Row> = self
            .inner
            .active()
            .into_iter()
            .filter(|r| r.credential.kind == kind)
            .collect();
        Ok(if rows.is_empty() {
            SignInState::SignedOut
        } else if rows.iter().any(|r| !r.meta.needs_sign_in) {
            SignInState::SignedIn
        } else {
            SignInState::NeedsSignIn
        })
    }

    pub async fn list(&self) -> Result<Vec<CredentialView>, CredError> {
        let mut rows = self.inner.active();
        rows.sort_by_key(|r| (r.credential.created_at, r.credential.id));
        Ok(rows.iter().map(view).collect())
    }

    /// Refreshes every active credential that expires within 5 minutes (Setup Assistant's background repair).
    pub async fn refresh_due(&self) -> Result<RefreshReport, CredError> {
        let mut report = RefreshReport::default();
        let due: Vec<Row> = self
            .inner
            .active()
            .into_iter()
            .filter(|r| {
                !r.meta.needs_sign_in
                    && !is_password_kind(r.credential.kind)
                    && expires_within_margin(r.credential.expires_at)
            })
            .collect();
        for row in due {
            match self.inner.token_of(&row).await {
                Ok(_) => report.refreshed += 1,
                Err(e) => match ordin_types::CodedError::code(&e) {
                    code::SIGN_IN_REQUIRED | code::KEYCHAIN_MISSING => {
                        report.needs_sign_in.push(row.credential.id)
                    }
                    _ => report.unreachable.push(row.credential.id),
                },
            }
        }
        Ok(report)
    }

    /// Deletes the secret, marks the row `revoked`. Idempotent on an already revoked row.
    pub async fn sign_out(&self, credential_id: CredentialId) -> Result<(), CredError> {
        let pool = self.inner.pool()?;
        match store::state_of(pool, credential_id).await?.as_deref() {
            None => return fail(code::NOT_FOUND),
            Some("revoked") => return Ok(()),
            Some(_) => {}
        }
        let row = self.inner.row(credential_id)?;
        self.inner.revoke_at_provider(&row).await;
        let name = SecretName::new(&row.credential.keychain_ref)?;
        self.inner.secrets.delete(&name).await?;
        store::revoke(pool, credential_id).await?;
        self.inner.forget(credential_id);
        self.inner.publish(&row.credential, SignInState::SignedOut);
        tracing::info!(
            target: "ordin_identity",
            credential_id = credential_id.to_string().as_str(),
            kind = kind_str(row.credential.kind),
            "cred.signed_out"
        );
        Ok(())
    }

    /// The active mail credential; `None` when the person has not set up mail.
    pub async fn mail_account(&self) -> Result<Option<CredentialView>, CredError> {
        let rows: Vec<Row> = self
            .inner
            .active()
            .into_iter()
            .filter(|r| is_mail_kind(r.credential.kind))
            .collect();
        match rows.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(view(one))),
            _ => fail(code::AMBIGUOUS),
        }
    }

    /// Who the person is, from the active credentials' labels (H.4). No network call.
    pub async fn person(&self) -> Result<PersonProfile, CredError> {
        let rows = self.inner.active();
        let first = |kind: CredentialKind| rows.iter().find(|r| r.credential.kind == kind);
        let github = first(CredentialKind::Github);
        let company = [
            CredentialKind::Oidc,
            CredentialKind::Microsoft,
            CredentialKind::Google,
        ]
        .into_iter()
        .filter_map(first)
        .collect::<Vec<_>>();
        let email = company.first().map(|r| r.credential.account_label.clone());
        let github_login = github.map(|r| r.credential.account_label.clone());
        let display = company
            .iter()
            .copied()
            .chain(github)
            .find_map(|r| r.meta.display_name.clone());
        let name = display
            .or_else(|| {
                email
                    .as_deref()
                    .and_then(|e| e.split('@').next())
                    .filter(|local| !local.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| github_login.clone());
        match name {
            Some(name) => Ok(PersonProfile {
                name,
                email,
                github_login,
            }),
            None => fail(code::SIGN_IN_REQUIRED),
        }
    }
}

pub(crate) fn view(row: &Row) -> CredentialView {
    CredentialView {
        id: row.credential.id,
        kind: row.credential.kind,
        account_label: row.credential.account_label.clone(),
        state: if row.meta.needs_sign_in {
            SignInState::NeedsSignIn
        } else {
            SignInState::SignedIn
        },
        expires_at: row.credential.expires_at,
    }
}

impl Inner {
    pub(crate) fn pool(&self) -> Result<&SqlitePool, CredError> {
        self.pool
            .as_ref()
            .ok_or_else(|| CredError::of(code::STORE_FAILED))
    }

    pub(crate) fn active(&self) -> Vec<Row> {
        self.rows
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    /// An active row by id (`CRED_NOT_FOUND` otherwise).
    pub(crate) fn row(&self, id: CredentialId) -> Result<Row, CredError> {
        self.rows
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()
            .ok_or_else(|| CredError::of(code::NOT_FOUND))
    }

    /// E.1: by `(kind, account_label)`, or the single active row of `kind`.
    pub(crate) fn select(
        &self,
        kind: CredentialKind,
        account_label: Option<&str>,
    ) -> Result<Row, CredError> {
        if is_password_kind(kind) {
            return fail(code::KIND_HAS_NO_TOKEN);
        }
        let mut rows: Vec<Row> = self
            .active()
            .into_iter()
            .filter(|r| r.credential.kind == kind)
            .filter(|r| account_label.is_none_or(|l| r.credential.account_label == l))
            .collect();
        match rows.len() {
            0 => fail(code::SIGN_IN_REQUIRED),
            1 => Ok(rows.remove(0)),
            _ => fail(code::AMBIGUOUS),
        }
    }

    pub(crate) fn refresh_lock(&self, id: CredentialId) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(lock(&self.refresh_locks).entry(id).or_default())
    }

    pub(crate) fn publish(&self, credential: &Credential, state: SignInState) {
        // No subscriber is not an error: the program subscribes when it wants the events.
        let _ = self.events.send(SignInChanged {
            credential_id: credential.id,
            credential_kind: credential.kind,
            state,
        });
    }

    /// Removes a credential from every in-memory map.
    pub(crate) fn forget(&self, id: CredentialId) {
        self.rows
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        lock(&self.tokens).remove(&id);
    }

    pub(crate) fn remember(&self, row: Row) {
        self.rows
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(row.credential.id, row);
    }

    /// Sets `needs_sign_in` in the table and in memory; publishes `NeedsSignIn` when `publish`.
    pub(crate) async fn mark_needs_sign_in(
        &self,
        row: &Row,
        reason: &str,
        refused_at: Option<Timestamp>,
        publish: bool,
    ) -> Result<(), CredError> {
        store::mark_needs_sign_in(self.pool()?, row.credential.id, reason, refused_at).await?;
        let mut rows = self.rows.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(stored) = rows.get_mut(&row.credential.id) {
            stored.meta.needs_sign_in = true;
            stored.meta.last_error_code = Some(reason.to_owned());
            if refused_at.is_some() {
                stored.meta.refused_at = refused_at;
            }
        }
        drop(rows);
        if publish {
            self.publish(&row.credential, SignInState::NeedsSignIn);
        }
        Ok(())
    }

    /// E.3: the token blob, from memory or the secret store. A missing item marks the credential.
    pub(crate) async fn read_blob(&self, row: &Row) -> Result<Arc<TokenBlob>, CredError> {
        if let Some(blob) = lock(&self.tokens).get(&row.credential.id) {
            return Ok(Arc::clone(blob));
        }
        let name = SecretName::new(&row.credential.keychain_ref)?;
        let Some(bytes) = self.secrets.get(&name).await? else {
            self.mark_needs_sign_in(row, code::KEYCHAIN_MISSING, None, false)
                .await?;
            return fail(code::KEYCHAIN_MISSING);
        };
        let blob: TokenBlob =
            serde_json::from_slice(&bytes).or_else(|_| fail(code::SECRET_STORE_CORRUPT))?;
        let blob = Arc::new(blob);
        lock(&self.tokens).insert(row.credential.id, Arc::clone(&blob));
        Ok(blob)
    }

    /// Writes a new blob under the credential's name and caches it.
    pub(crate) async fn write_blob(
        &self,
        row: &Row,
        blob: TokenBlob,
    ) -> Result<Arc<TokenBlob>, CredError> {
        let name = SecretName::new(&row.credential.keychain_ref)?;
        self.secrets.put(&name, &blob.to_bytes()?).await?;
        let blob = Arc::new(blob);
        lock(&self.tokens).insert(row.credential.id, Arc::clone(&blob));
        Ok(blob)
    }

    /// E.2–E.5 for one selected row.
    pub(crate) async fn token_of(&self, row: &Row) -> Result<AccessToken, CredError> {
        if row.meta.needs_sign_in {
            return fail(code::SIGN_IN_REQUIRED);
        }
        let gate = self.refresh_lock(row.credential.id);
        let _one = gate.lock().await;
        // A caller that waited for another's refresh reads the row it left behind.
        let row = self.row(row.credential.id).unwrap_or_else(|_| row.clone());
        if row.meta.needs_sign_in {
            return fail(code::SIGN_IN_REQUIRED);
        }
        let mut blob = self.read_blob(&row).await?;
        if expires_within_margin(blob.expires_at) {
            blob = self.refresh(&row, &blob).await?;
        }
        Ok(AccessToken {
            token: SecretString::new(blob.access_token.clone()),
            expires_at: blob.expires_at,
            credential_id: row.credential.id,
        })
    }

    /// E.4: one refresh-token grant. The caller holds the credential's refresh lock.
    pub(crate) async fn refresh(
        &self,
        row: &Row,
        blob: &Arc<TokenBlob>,
    ) -> Result<Arc<TokenBlob>, CredError> {
        let (Some(refresh_token), Some(endpoint)) = (&blob.refresh_token, &blob.token_endpoint)
        else {
            // No refresh token: a token still valid is returned as it is; an expired one needs a sign-in.
            if expired(blob.expires_at) {
                return fail(code::SIGN_IN_REQUIRED);
            }
            return Ok(Arc::clone(blob));
        };
        let client_id = blob.client_id.clone().unwrap_or_default();
        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &client_id),
        ];
        if let Some(secret) = &blob.client_secret {
            form.push(("client_secret", secret));
        }
        let reply = self
            .http
            .post_form(endpoint, &form, None, self.http.timeout)
            .await;
        let tokens = match reply.and_then(oauth::read_tokens) {
            Ok(Outcome::Tokens(tokens)) => tokens,
            Ok(Outcome::Refused { error, status }) => {
                return Err(self.refresh_failed(row, &error, status).await)
            }
            Err(e) => {
                self.log_refresh_failed(row, ordin_types::CodedError::code(&e));
                return Err(e);
            }
        };
        let mut next = (**blob).clone();
        next.access_token = tokens.access_token.to_string();
        if let Some(rotated) = &tokens.refresh_token {
            next.refresh_token = Some(rotated.to_string());
        }
        next.expires_at = tokens.expires_at;
        if let Some(scopes) = tokens.scopes.clone() {
            next.scopes = scopes;
        }
        let next = self.write_blob(row, next).await?;
        let refreshed_at = Timestamp::now();
        store::record_refresh(
            self.pool()?,
            row.credential.id,
            next.expires_at,
            refreshed_at,
        )
        .await?;
        {
            let mut rows = self.rows.write().unwrap_or_else(PoisonError::into_inner);
            if let Some(stored) = rows.get_mut(&row.credential.id) {
                stored.credential.expires_at = next.expires_at;
                stored.credential.refreshed_at = Some(refreshed_at);
            }
        }
        tracing::debug!(
            target: "ordin_identity",
            credential_id = row.credential.id.to_string().as_str(),
            kind = kind_str(row.credential.kind),
            expires_at = next.expires_at.map(|t| t.to_wire()).unwrap_or_default().as_str(),
            "cred.refreshed"
        );
        Ok(next)
    }

    /// A refused refresh: `invalid_grant` or 400/401 marks the credential and is `CRED_SIGN_IN_REQUIRED`;
    /// any other refusal is `CRED_PROVIDER_UNREACHABLE` with `needs_sign_in` unchanged.
    pub(crate) async fn refresh_failed(&self, row: &Row, error: &str, status: u16) -> CredError {
        if oauth::refresh_refused(error, status) {
            self.log_refresh_failed(row, code::REFRESH_REFUSED);
            if let Err(e) = self
                .mark_needs_sign_in(row, code::REFRESH_REFUSED, None, true)
                .await
            {
                return e;
            }
            lock(&self.tokens).remove(&row.credential.id);
            return CredError::of(code::SIGN_IN_REQUIRED);
        }
        self.log_refresh_failed(row, code::PROVIDER_UNREACHABLE);
        CredError::of(code::PROVIDER_UNREACHABLE)
    }

    pub(crate) fn log_refresh_failed(&self, row: &Row, failure: &str) {
        tracing::warn!(
            target: "ordin_identity",
            credential_id = row.credential.id.to_string().as_str(),
            kind = kind_str(row.credential.kind),
            code = failure,
            "cred.refresh_failed"
        );
    }

    /// G.2: the provider's revocation call where one exists (Google, Slack), 5-second timeout; a failure
    /// is logged only.
    async fn revoke_at_provider(&self, row: &Row) {
        let endpoint = match row.credential.kind {
            CredentialKind::Google => "https://oauth2.googleapis.com/revoke",
            CredentialKind::Slack => "https://slack.com/api/auth.revoke",
            _ => return,
        };
        let Ok(blob) = self.read_blob(row).await else {
            return;
        };
        let reply = match row.credential.kind {
            CredentialKind::Google => {
                self.http
                    .post_form(
                        endpoint,
                        &[("token", &blob.access_token)],
                        None,
                        oauth::REVOKE_TIMEOUT,
                    )
                    .await
            }
            _ => {
                self.http
                    .post_form(
                        endpoint,
                        &[],
                        Some(&blob.access_token),
                        oauth::REVOKE_TIMEOUT,
                    )
                    .await
            }
        };
        let done = reply.is_ok_and(|r| (200..300).contains(&r.status));
        if !done {
            // SPEC-GAP-4: G.2 says a failed revocation is logged but names no event; this name is
            // chosen here.
            tracing::warn!(
                target: "ordin_identity",
                credential_id = row.credential.id.to_string().as_str(),
                kind = kind_str(row.credential.kind),
                "cred.revoke_failed"
            );
        }
    }

    /// C.10 and F.1: writes the secret first, then the rows; a row failure after a new secret deletes
    /// that secret (no orphan). Refuses a record that holds the secret (`CRED_SECRET_IN_RECORD`).
    pub(crate) async fn persist(
        &self,
        row: Row,
        is_new: bool,
        secret: &[u8],
        previous_ref: Option<String>,
    ) -> Result<Credential, CredError> {
        guard_no_secret(&row, secret)?;
        let pool = self.pool()?;
        let name = SecretName::new(&row.credential.keychain_ref)?;
        self.secrets.put(&name, secret).await?;
        if let Err(e) = store::upsert(pool, &row, is_new).await {
            if is_new {
                let _ = self.secrets.delete(&name).await;
            }
            return Err(e);
        }
        if let Some(previous) = previous_ref.filter(|p| *p != row.credential.keychain_ref) {
            if let Ok(old) = SecretName::new(&previous) {
                let _ = self.secrets.delete(&old).await;
            }
        }
        lock(&self.tokens).remove(&row.credential.id);
        let mut stored = row.clone();
        stored.meta.needs_sign_in = false;
        stored.meta.refused_at = None;
        self.remember(stored);
        self.publish(&row.credential, SignInState::SignedIn);
        Ok(row.credential)
    }
}

/// `CRED_SECRET_IN_RECORD`: no field of the record may hold the secret, or the token inside a blob.
fn guard_no_secret(row: &Row, secret: &[u8]) -> Result<(), CredError> {
    let mut needles: Vec<String> = Vec::new();
    if let Ok(blob) = serde_json::from_slice::<TokenBlob>(secret) {
        needles.push(blob.access_token.clone());
        if let Some(refresh) = &blob.refresh_token {
            needles.push(refresh.clone());
        }
    } else if let Ok(text) = std::str::from_utf8(secret) {
        needles.push(text.to_owned());
    }
    let c = &row.credential;
    let m = &row.meta;
    let mut fields: Vec<&str> = vec![&c.account_label, &c.keychain_ref, &m.provider_host];
    fields.extend(m.display_name.as_deref());
    fields.extend(m.scopes.iter().map(String::as_str));
    let leaked = needles
        .iter()
        .filter(|n| !n.is_empty())
        .any(|n| fields.iter().any(|f| f.contains(n.as_str())));
    for mut n in needles {
        n.zeroize();
    }
    if leaked {
        return fail(code::SECRET_IN_RECORD);
    }
    Ok(())
}

/// A new active row with its meta.
pub(crate) fn new_row(
    id: CredentialId,
    kind: CredentialKind,
    account_label: String,
    created_at: Timestamp,
    expires_at: Option<Timestamp>,
    meta: store::Meta,
) -> Row {
    Row {
        credential: Credential {
            id,
            kind,
            keychain_ref: credential_secret_name(kind, id),
            account_label,
            expires_at,
            state: CredentialState::Active,
            created_at,
            refreshed_at: None,
        },
        meta,
    }
}
