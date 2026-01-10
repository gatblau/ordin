//! Sign-in (Internal Logic C, 5a; §J.1–J.2): request checks, endpoints, the browser, device and relayed
//! methods, the token exchange and the credential it leaves.

use std::sync::Arc;
use std::time::Duration;

use ordin_types::domain::identity::{Credential, CredentialKind};
use ordin_types::text::{check_https_url, AreaCodes};
use ordin_types::time::Timestamp;
use ordin_types::CodedError as _;
use ordin_types::CredentialId;
use serde_json::Value;
use subtle::ConstantTimeEq as _;
use tokio::sync::watch;
use zeroize::Zeroizing;

use crate::config::Program;
use crate::error::{code, fail, CredError};
use crate::loopback::{self, Callback};
use crate::oauth::{self, Auth, Outcome, Tokens, DEVICE_GRANT_TYPE};
use crate::secret::SecretString;
use crate::service::{lock, new_row, short_hash, IdentityService, Inner, TokenBlob};
use crate::store::{kind_str, Meta, Row};
use crate::types::{
    ProviderConfig, SignInChallenge, SignInChallengeId, SignInMethod, SignInRequest,
    JIRA_OAUTH_SCOPES,
};

/// At most this many sign-ins are pending at once (C.12).
const MAX_PENDING: usize = 4;
/// Three state mismatches end a relayed challenge (C.5a, chosen there).
const MAX_RELAY_MISMATCHES: u8 = 3;
/// Slack user scopes of every Slack sign-in (SA-SLACK-01).
const SLACK_SCOPES: [&str; 5] = [
    "chat:write",
    "channels:history",
    "groups:history",
    "channels:read",
    "groups:read",
];
/// GitLab's scopes (SA-CLOSE-01).
pub(crate) const GITLAB_SCOPES: [&str; 3] = ["read_repository", "write_repository", "api"];
/// A wait for a device or browser result ends this long after the challenge's expiry, so the flow's
/// own `CRED_SIGN_IN_TIMEOUT` arrives first.
const WAIT_GRACE: Duration = Duration::from_secs(1);

const URL_CODES: AreaCodes = AreaCodes {
    name_empty: code::URL_INVALID,
    name_too_long: code::URL_INVALID,
    path_invalid: code::URL_INVALID,
    url_invalid: code::URL_INVALID,
};

type Outcome2 = Option<Result<Credential, &'static str>>;

/// One open sign-in (held in memory only).
pub(crate) struct Pending {
    method: SignInMethod,
    expires_at: Timestamp,
    outcome: watch::Sender<Outcome2>,
    task: Option<tokio::task::AbortHandle>,
    relay: Option<Relay>,
}

impl Pending {
    fn ended(&self) -> bool {
        self.outcome.borrow().is_some()
    }
}

/// What a relayed challenge keeps on the server (C.5a): the verifier never leaves it.
struct Relay {
    flow: Arc<Flow>,
    verifier: Zeroizing<String>,
    state: Zeroizing<String>,
    redirect_uri: String,
    mismatches: u8,
    exchanging: bool,
}

/// Everything settled about one sign-in before the person is asked.
pub(crate) struct Flow {
    kind: CredentialKind,
    provider: ProviderConfig,
    client_id: String,
    client_secret: Option<String>,
    /// The scopes requested now: the union of stored and requested (C.3).
    scopes: Vec<String>,
    authorize: Option<String>,
    token: String,
    device: Option<String>,
    /// Extra authorisation parameters (`audience`, `prompt`, `team`, `access_type`).
    extra: Vec<(&'static str, String)>,
    /// `scope`, or Slack's `user_scope`.
    scope_param: &'static str,
    provider_host: String,
    existing: Option<Row>,
    /// Confluence and Jira: the site URL whose resource names the label.
    site: Option<url::Url>,
    challenge_id: SignInChallengeId,
}

fn union(into: &mut Vec<String>, more: impl IntoIterator<Item = String>) {
    for scope in more {
        if !into.contains(&scope) {
            into.push(scope);
        }
    }
}

fn owned(scopes: &[&str]) -> Vec<String> {
    scopes.iter().map(|s| (*s).to_owned()).collect()
}

/// C.1: a client id of 1–256 characters.
fn client_id_ok(client_id: &str) -> Result<(), CredError> {
    let chars = client_id.chars().count();
    if chars == 0 || chars > 256 {
        return fail(code::PROVIDER_NOT_CONFIGURED);
    }
    Ok(())
}

/// A host name with no scheme, path, port or user part, 1–255 characters.
pub(crate) fn host_ok(host: &str) -> Result<(), CredError> {
    let plain = !host.is_empty()
        && host.len() <= 255
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if !plain || url::Host::parse(host).is_err() {
        return fail(code::URL_INVALID);
    }
    Ok(())
}

/// §J: lower case `{name}.atlassian.net`, 1–255 characters, no scheme, path or port.
pub(crate) fn jira_host_ok(host: &str) -> Result<(), CredError> {
    let name = host.strip_suffix(".atlassian.net").unwrap_or_default();
    let ok = !name.is_empty()
        && host.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !ok {
        return fail(code::URL_INVALID);
    }
    Ok(())
}

fn https(raw: &str) -> Result<url::Url, CredError> {
    check_https_url(raw, false, URL_CODES).or_else(|_| fail(code::URL_INVALID))
}

/// The site host of a Confluence or Jira site URL.
fn site_host(site: &url::Url) -> String {
    site.host_str().unwrap_or_default().to_owned()
}

impl IdentityService {
    pub async fn begin_sign_in(
        &self,
        request: SignInRequest,
    ) -> Result<SignInChallenge, CredError> {
        let inner = &self.inner;
        let kind = request.provider.kind();
        // C.1: values.
        let (client_id, site) = check_request(inner, &request)?;
        // C.2: flows this program offers.
        check_flow(inner, &request)?;
        // C.12: the cap, counting only challenges still open.
        {
            let mut pending = lock(&inner.pending);
            let now = Timestamp::now();
            // An ended challenge is kept for its `wait_for_sign_in` until it expires.
            pending.retain(|_, p| p.expires_at >= now || (!p.ended() && p.task.is_some()));
            if pending.values().filter(|p| !p.ended()).count() >= MAX_PENDING {
                return fail(code::TOO_MANY_SIGN_INS);
            }
        }
        // C.3: a credential that already covers the request.
        let fixed: Vec<String> = match &request.provider {
            ProviderConfig::Slack { .. } => owned(&SLACK_SCOPES),
            ProviderConfig::Gitlab { .. } => owned(&GITLAB_SCOPES),
            ProviderConfig::Jira { .. } => owned(JIRA_OAUTH_SCOPES),
            ProviderConfig::Oidc { scopes, .. } | ProviderConfig::MailServerSso { scopes, .. } => {
                scopes.clone()
            }
            _ => Vec::new(),
        };
        let mut requested = fixed;
        union(&mut requested, request.scopes.iter().cloned());
        let existing = inner.active().into_iter().find(|r| {
            r.credential.kind == kind
                && match &request.provider {
                    ProviderConfig::MailServerSso { mail_account, .. } => {
                        r.credential.account_label == *mail_account
                    }
                    _ => true,
                }
        });
        if let (Some(row), Some(site)) = (&existing, &site) {
            // §J.2: one Jira site per person.
            if kind == CredentialKind::Jira && row.credential.account_label != site_host(site) {
                return fail(code::DUPLICATE);
            }
        }
        if let Some(row) = &existing {
            let covered = requested.iter().all(|s| row.meta.scopes.contains(s));
            if covered && !row.meta.needs_sign_in {
                return Ok(SignInChallenge::AlreadySignedIn {
                    credential: row.credential.clone(),
                });
            }
        }
        let mut scopes = existing
            .as_ref()
            .map(|r| r.meta.scopes.clone())
            .unwrap_or_default();
        union(&mut scopes, requested);
        // C.4: endpoints.
        let challenge_id = SignInChallengeId(uuid::Uuid::now_v7());
        let flow = resolve(
            inner,
            &request,
            kind,
            client_id,
            site,
            scopes,
            existing,
            challenge_id,
        )
        .await?;
        tracing::info!(
            target: "ordin_identity",
            challenge_id = challenge_id.0.to_string().as_str(),
            kind = kind_str(kind),
            method = request.method.as_str(),
            "cred.sign_in.started"
        );
        let flow = Arc::new(flow);
        let device_only = matches!(kind, CredentialKind::Github | CredentialKind::Gitlab);
        if device_only || request.method == SignInMethod::DeviceCode {
            self.start_device(flow, request.method).await
        } else if request.method == SignInMethod::RelayedBrowser {
            self.start_relayed(flow, request.relay_port.unwrap_or_default())
        } else {
            self.start_browser(flow).await
        }
    }

    /// Resolves when the provider issues tokens, the person cancels, or the challenge expires.
    pub async fn wait_for_sign_in(
        &self,
        challenge_id: SignInChallengeId,
    ) -> Result<Credential, CredError> {
        let (mut receiver, expires_at) = {
            let pending = lock(&self.inner.pending);
            let entry = pending
                .get(&challenge_id)
                .ok_or_else(|| CredError::of(code::CHALLENGE_UNKNOWN))?;
            (entry.outcome.subscribe(), entry.expires_at)
        };
        let left = (expires_at.unix_millis() - Timestamp::now().unix_millis()).max(0);
        let deadline = Duration::from_millis(left as u64) + WAIT_GRACE;
        let result = match tokio::time::timeout(deadline, receiver.wait_for(Option::is_some)).await
        {
            Ok(Ok(outcome)) => outcome.clone().unwrap_or(Err(code::SIGN_IN_TIMEOUT)),
            Ok(Err(_)) => Err(code::SIGN_IN_CANCELLED),
            Err(_) => Err(code::SIGN_IN_TIMEOUT),
        };
        if let Some(ended) = lock(&self.inner.pending).remove(&challenge_id) {
            if let Some(task) = ended.task {
                task.abort();
            }
        }
        result.map_err(CredError::of)
    }

    pub fn cancel_sign_in(&self, challenge_id: SignInChallengeId) -> Result<(), CredError> {
        let mut pending = lock(&self.inner.pending);
        let entry = pending
            .get_mut(&challenge_id)
            .ok_or_else(|| CredError::of(code::CHALLENGE_UNKNOWN))?;
        if let Some(task) = entry.task.take() {
            task.abort();
        }
        entry.relay = None;
        if !entry.ended() {
            let _ = entry
                .outcome
                .send_replace(Some(Err(code::SIGN_IN_CANCELLED)));
        }
        Ok(())
    }

    /// Personal server only. Exchanges the code a paired desktop received for the challenge's tokens.
    // spec: ordin-desktop/components/IdentityAndCredentials#SA-RECON-07
    pub async fn complete_relayed_sign_in(
        &self,
        challenge_id: SignInChallengeId,
        code_in: SecretString,
        state: String,
    ) -> Result<(), CredError> {
        let inner = &self.inner;
        let (flow, verifier, redirect_uri) = {
            let mut pending = lock(&inner.pending);
            let entry = match pending.get_mut(&challenge_id) {
                Some(entry) if !entry.ended() => entry,
                _ => return fail(code::CHALLENGE_UNKNOWN),
            };
            if entry.method != SignInMethod::RelayedBrowser {
                return fail(code::FLOW_NOT_OFFERED);
            }
            if entry.expires_at < Timestamp::now() {
                let _ = entry.outcome.send_replace(Some(Err(code::SIGN_IN_TIMEOUT)));
                return fail(code::SIGN_IN_TIMEOUT);
            }
            let Some(relay) = entry.relay.as_mut() else {
                return fail(code::CHALLENGE_UNKNOWN);
            };
            if relay.exchanging {
                return fail(code::CHALLENGE_UNKNOWN);
            }
            let matches = bool::from(state.as_bytes().ct_eq(relay.state.as_bytes()));
            if !matches {
                relay.mismatches += 1;
                if relay.mismatches >= MAX_RELAY_MISMATCHES {
                    let _ = entry
                        .outcome
                        .send_replace(Some(Err(code::RELAY_STATE_MISMATCH)));
                    entry.relay = None;
                }
                return fail(code::RELAY_STATE_MISMATCH);
            }
            relay.exchanging = true;
            (
                Arc::clone(&relay.flow),
                relay.verifier.clone(),
                relay.redirect_uri.clone(),
            )
        };
        tracing::info!(
            target: "ordin_identity",
            challenge_id = challenge_id.0.to_string().as_str(),
            kind = kind_str(flow.kind),
            "cred.sign_in.relayed"
        );
        let result = async {
            let tokens =
                exchange_code(inner, &flow, code_in.expose(), &verifier, &redirect_uri).await?;
            finish(inner, &flow, tokens, "browser").await
        }
        .await;
        let reply = result
            .as_ref()
            .map(|_| ())
            .map_err(|e| CredError::of(e.code()));
        end(inner, &flow, result);
        reply
    }

    /// C.5: the browser method — loopback listener, PKCE, the authorisation URL.
    async fn start_browser(&self, flow: Arc<Flow>) -> Result<SignInChallenge, CredError> {
        let inner = &self.inner;
        let (listener, port) = loopback::bind().await?;
        let pkce = oauth::pkce()?;
        let redirect_uri = loopback::redirect_uri(port);
        let url = authorisation_url(&flow, &pkce.challenge, &pkce.state, &redirect_uri)?;
        open_browser(inner, &url)?;
        let expires_at = after(inner.sign_in_wait);
        let (sender, _) = watch::channel(None);
        let task_inner = Arc::clone(inner);
        let task_flow = Arc::clone(&flow);
        let task = tokio::spawn(async move {
            let run = async {
                let mut accepted = loopback::accept(&listener, &pkce.state).await;
                match accepted.callback {
                    Callback::Denied => {
                        loopback::respond(&mut accepted.stream, 400, "").await;
                        Err(CredError::of(code::SIGN_IN_CANCELLED))
                    }
                    Callback::Error => {
                        loopback::respond(&mut accepted.stream, 400, "").await;
                        Err(CredError::of(code::PROVIDER_REFUSED))
                    }
                    Callback::Code(code_in) => {
                        let tokens = exchange_code(
                            &task_inner,
                            &task_flow,
                            &code_in,
                            &pkce.verifier,
                            &redirect_uri,
                        )
                        .await;
                        let done = match tokens {
                            Ok(tokens) => finish(&task_inner, &task_flow, tokens, "browser").await,
                            Err(e) => Err(e),
                        };
                        if done.is_ok() {
                            loopback::signed_in(&mut accepted.stream).await;
                        } else {
                            loopback::respond(&mut accepted.stream, 400, "").await;
                        }
                        done
                    }
                }
            };
            let result = match tokio::time::timeout(task_inner.sign_in_wait, run).await {
                Ok(result) => result,
                Err(_) => Err(CredError::of(code::SIGN_IN_TIMEOUT)),
            };
            end(&task_inner, &task_flow, result);
        });
        self.register(
            &flow,
            SignInMethod::Browser,
            expires_at,
            sender,
            Some(task.abort_handle()),
            None,
        );
        Ok(SignInChallenge::BrowserOpened {
            challenge_id: flow.challenge_id,
            expires_at,
        })
    }

    /// C.5a: the relayed browser method — no port bound, the verifier and `state` kept here.
    fn start_relayed(
        &self,
        flow: Arc<Flow>,
        relay_port: u16,
    ) -> Result<SignInChallenge, CredError> {
        let pkce = oauth::pkce()?;
        let redirect_uri = loopback::redirect_uri(relay_port);
        let url = authorisation_url(&flow, &pkce.challenge, &pkce.state, &redirect_uri)?;
        let expires_at = after(self.inner.sign_in_wait);
        let (sender, _) = watch::channel(None);
        let relay = Relay {
            flow: Arc::clone(&flow),
            verifier: pkce.verifier,
            state: pkce.state,
            redirect_uri,
            mismatches: 0,
            exchanging: false,
        };
        self.register(
            &flow,
            SignInMethod::RelayedBrowser,
            expires_at,
            sender,
            None,
            Some(relay),
        );
        Ok(SignInChallenge::RelayedBrowser {
            challenge_id: flow.challenge_id,
            authorisation_url: url,
            expires_at,
        })
    }

    /// C.6: the device method; GitHub and GitLab use it whatever `method` says, opening the browser at
    /// the verification page on the desktop (SA-CRED-11, SA-CLOSE-01).
    async fn start_device(
        &self,
        flow: Arc<Flow>,
        method: SignInMethod,
    ) -> Result<SignInChallenge, CredError> {
        let inner = &self.inner;
        let Some(endpoint) = &flow.device else {
            return fail(code::FLOW_NOT_OFFERED);
        };
        let scope = flow.scopes.join(" ");
        let mut form: Vec<(&str, &str)> = vec![("client_id", &flow.client_id)];
        if !scope.is_empty() {
            form.push(("scope", &scope));
        }
        let reply = inner
            .http
            .post_form(endpoint, &form, None, inner.http.timeout)
            .await?;
        let grant = oauth::read_device_grant(reply)?;
        if method == SignInMethod::Browser {
            let page = match &flow.provider {
                ProviderConfig::Github { host } => format!("https://{host}/login/device"),
                _ => grant.verification_uri.clone(),
            };
            open_browser(inner, &page)?;
        }
        let (sender, _) = watch::channel(None);
        let expires_at = grant.expires_at;
        let challenge = SignInChallenge::DeviceCode {
            challenge_id: flow.challenge_id,
            verification_uri: grant.verification_uri.clone(),
            user_code: grant.user_code.to_string(),
            expires_at,
        };
        let task_inner = Arc::clone(inner);
        let task_flow = Arc::clone(&flow);
        let task = tokio::spawn(async move {
            let result = match poll_device(&task_inner, &task_flow, &grant).await {
                Ok(tokens) => finish(&task_inner, &task_flow, tokens, "device_code").await,
                Err(e) => Err(e),
            };
            end(&task_inner, &task_flow, result);
        });
        self.register(
            &flow,
            method,
            expires_at,
            sender,
            Some(task.abort_handle()),
            None,
        );
        Ok(challenge)
    }

    fn register(
        &self,
        flow: &Flow,
        method: SignInMethod,
        expires_at: Timestamp,
        outcome: watch::Sender<Outcome2>,
        task: Option<tokio::task::AbortHandle>,
        relay: Option<Relay>,
    ) {
        lock(&self.inner.pending).insert(
            flow.challenge_id,
            Pending {
                method,
                expires_at,
                outcome,
                task,
                relay,
            },
        );
    }
}

fn after(wait: Duration) -> Timestamp {
    let at =
        Timestamp::now().unix_millis() + i64::try_from(wait.as_millis()).unwrap_or(i64::MAX / 2);
    Timestamp::from_unix_millis(at).unwrap_or_else(|_| Timestamp::now())
}

fn open_browser(inner: &Inner, url: &str) -> Result<(), CredError> {
    let Some(browser) = &inner.browser else {
        return fail(code::FLOW_NOT_OFFERED);
    };
    browser.open(url).or_else(|_| fail(code::BROWSER_FAILED))
}

/// Records how the flow ended, logs a failure, and wakes `wait_for_sign_in`.
fn end(inner: &Inner, flow: &Flow, result: Result<Credential, CredError>) {
    if let Err(e) = &result {
        tracing::warn!(
            target: "ordin_identity",
            challenge_id = flow.challenge_id.0.to_string().as_str(),
            kind = kind_str(flow.kind),
            code = e.code(),
            "cred.sign_in.failed"
        );
    }
    let mut pending = lock(&inner.pending);
    if let Some(entry) = pending.get_mut(&flow.challenge_id) {
        entry.task = None;
        entry.relay = None;
        if !entry.ended() {
            let _ = entry
                .outcome
                .send_replace(Some(result.map_err(|e| e.code())));
        }
    }
}

/// C.1: the request's values. Returns the client id and, for Confluence and Jira, the site URL.
fn check_request(
    inner: &Inner,
    request: &SignInRequest,
) -> Result<(String, Option<url::Url>), CredError> {
    match &request.provider {
        ProviderConfig::Github { host } => {
            host_ok(host)?;
            let id = inner
                .github_app_client_id
                .clone()
                .ok_or_else(|| CredError::of(code::PROVIDER_NOT_CONFIGURED))?;
            client_id_ok(&id)?;
            Ok((id, None))
        }
        ProviderConfig::Gitlab { host, client_id } => {
            host_ok(host)?;
            client_id_ok(client_id)?;
            Ok((client_id.clone(), None))
        }
        ProviderConfig::Oidc {
            issuer, client_id, ..
        }
        | ProviderConfig::MailServerSso {
            issuer, client_id, ..
        } => {
            https(issuer)?;
            client_id_ok(client_id)?;
            Ok((client_id.clone(), None))
        }
        ProviderConfig::Microsoft {
            tenant_id,
            client_id,
        } => {
            client_id_ok(client_id)?;
            client_id_ok(tenant_id)?;
            Ok((client_id.clone(), None))
        }
        ProviderConfig::Google { client_id, .. } | ProviderConfig::Slack { client_id, .. } => {
            client_id_ok(client_id)?;
            Ok((client_id.clone(), None))
        }
        ProviderConfig::Confluence { site, client_id } => {
            let url = https(site)?;
            client_id_ok(client_id)?;
            Ok((client_id.clone(), Some(url)))
        }
        ProviderConfig::Jira { site, client_id } => {
            let url = https(site)?;
            jira_host_ok(&site_host(&url))?;
            if url.port().is_some() || !matches!(url.path(), "" | "/") {
                return fail(code::URL_INVALID);
            }
            client_id_ok(client_id)?;
            Ok((client_id.clone(), Some(url)))
        }
    }
}

/// C.2 and §J.1: refuse a flow the program cannot offer.
fn check_flow(inner: &Inner, request: &SignInRequest) -> Result<(), CredError> {
    let program = inner.program;
    if matches!(
        program,
        Program::AgentSingleShape | Program::AgentScaledShape
    ) {
        return fail(code::FLOW_NOT_OFFERED);
    }
    let provider = &request.provider;
    if matches!(provider, ProviderConfig::Jira { .. }) && program != Program::Desktop {
        return fail(code::FLOW_NOT_OFFERED);
    }
    let refused = match request.method {
        SignInMethod::Browser => inner.browser.is_none(),
        SignInMethod::DeviceCode => matches!(
            provider,
            ProviderConfig::Slack { .. }
                | ProviderConfig::Confluence { .. }
                | ProviderConfig::Google { .. }
                | ProviderConfig::Jira { .. }
        ),
        SignInMethod::RelayedBrowser => {
            program != Program::PersonalServer
                || !matches!(provider, ProviderConfig::Slack { .. })
                || !request
                    .relay_port
                    .is_some_and(|p| loopback::PORTS.contains(&p))
        }
    };
    if refused {
        return fail(code::FLOW_NOT_OFFERED);
    }
    Ok(())
}

/// C.4: endpoints and the scopes and parameters of each provider.
#[allow(clippy::too_many_arguments)]
async fn resolve(
    inner: &Inner,
    request: &SignInRequest,
    kind: CredentialKind,
    client_id: String,
    site: Option<url::Url>,
    mut scopes: Vec<String>,
    existing: Option<Row>,
    challenge_id: SignInChallengeId,
) -> Result<Flow, CredError> {
    let mut flow = Flow {
        kind,
        provider: request.provider.clone(),
        client_id,
        client_secret: None,
        scopes: Vec::new(),
        authorize: None,
        token: String::new(),
        device: None,
        extra: Vec::new(),
        scope_param: "scope",
        provider_host: String::new(),
        existing,
        site,
        challenge_id,
    };
    // SPEC-GAP-6: C.8 reads the label of OIDC, Microsoft, Google and mail single sign-on from the ID
    // token, which these providers issue only for the `openid` scope (Microsoft's `preferred_username`
    // and Google's `email` need `profile` and `email`). C.5 adds only `offline_access`; these scopes are
    // added as well so the label C.8 needs is issued.
    let id_token_scopes: &[&str] = match kind {
        CredentialKind::Microsoft | CredentialKind::Google => &["openid", "profile", "email"],
        CredentialKind::Oidc | CredentialKind::MailServerSingleSignOn => &["openid"],
        _ => &[],
    };
    let mut extra_scopes = owned(id_token_scopes);
    match &request.provider {
        ProviderConfig::Github { host } => {
            flow.device = Some(format!("https://{host}/login/device/code"));
            flow.token = format!("https://{host}/login/oauth/access_token");
            flow.provider_host = host.clone();
        }
        ProviderConfig::Gitlab { host, .. } => {
            flow.device = Some(format!("https://{host}/oauth/authorize_device"));
            flow.token = format!("https://{host}/oauth/token");
            flow.provider_host = host.clone();
            scopes = owned(&GITLAB_SCOPES);
        }
        ProviderConfig::Oidc { issuer, .. } | ProviderConfig::MailServerSso { issuer, .. } => {
            let discovered = discover(inner, issuer).await?;
            flow.authorize = discovered.0;
            flow.token = discovered.1;
            flow.device = discovered.2;
            if request.method == SignInMethod::DeviceCode && flow.device.is_none() {
                return fail(code::FLOW_NOT_OFFERED);
            }
            flow.provider_host = issuer.clone();
            extra_scopes.push("offline_access".to_owned());
        }
        ProviderConfig::Microsoft { tenant_id, .. } => {
            let base = microsoft_base(tenant_id)?;
            flow.authorize = Some(format!("{base}/authorize"));
            flow.token = format!("{base}/token");
            flow.device = Some(format!("{base}/devicecode"));
            flow.provider_host = "login.microsoftonline.com".to_owned();
            extra_scopes.push("offline_access".to_owned());
        }
        ProviderConfig::Google { client_secret, .. } => {
            flow.authorize = Some("https://accounts.google.com/o/oauth2/v2/auth".to_owned());
            flow.token = "https://oauth2.googleapis.com/token".to_owned();
            flow.client_secret = client_secret.clone();
            flow.extra.push(("access_type", "offline".to_owned()));
            flow.provider_host = "accounts.google.com".to_owned();
        }
        ProviderConfig::Slack { workspace, .. } => {
            flow.authorize = Some("https://slack.com/oauth/v2/authorize".to_owned());
            flow.token = "https://slack.com/api/oauth.v2.access".to_owned();
            flow.scope_param = "user_scope";
            flow.extra.push(("team", workspace.clone()));
            flow.provider_host = "slack.com".to_owned();
        }
        ProviderConfig::Confluence { .. } | ProviderConfig::Jira { .. } => {
            flow.authorize = Some("https://auth.atlassian.com/authorize".to_owned());
            flow.token = "https://auth.atlassian.com/oauth/token".to_owned();
            flow.extra
                .push(("audience", "api.atlassian.com".to_owned()));
            flow.extra.push(("prompt", "consent".to_owned()));
            flow.provider_host = flow.site.as_ref().map(site_host).unwrap_or_default();
            extra_scopes.push("offline_access".to_owned());
        }
    }
    union(&mut scopes, extra_scopes);
    flow.scopes = scopes;
    Ok(flow)
}

/// `https://login.microsoftonline.com/<tenant_id>/oauth2/v2.0`, the tenant as one path segment.
fn microsoft_base(tenant_id: &str) -> Result<String, CredError> {
    let mut url = url::Url::parse("https://login.microsoftonline.com/")
        .or_else(|_| fail(code::PROVIDER_NOT_CONFIGURED))?;
    url.path_segments_mut()
        .map_err(|_| CredError::of(code::PROVIDER_NOT_CONFIGURED))?
        .clear()
        .extend([tenant_id, "oauth2", "v2.0"]);
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// C.4: OIDC discovery; the discovered `issuer` must equal the configured one.
async fn discover(
    inner: &Inner,
    issuer: &str,
) -> Result<(Option<String>, String, Option<String>), CredError> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let reply = inner.http.get(&url, Auth::None).await?;
    if !(200..300).contains(&reply.status) {
        return fail(code::PROVIDER_UNREACHABLE);
    }
    let text = |key: &str| {
        reply
            .body
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    if text("issuer").as_deref() != Some(issuer) {
        return fail(code::PROVIDER_REFUSED);
    }
    let token = text("token_endpoint").ok_or_else(|| CredError::of(code::PROVIDER_REFUSED))?;
    Ok((
        text("authorization_endpoint"),
        token,
        text("device_authorization_endpoint"),
    ))
}

/// C.5: the authorisation URL of the code flow.
fn authorisation_url(
    flow: &Flow,
    challenge: &str,
    state: &str,
    redirect_uri: &str,
) -> Result<String, CredError> {
    let Some(authorize) = &flow.authorize else {
        return fail(code::FLOW_NOT_OFFERED);
    };
    let mut url = url::Url::parse(authorize).or_else(|_| fail(code::PROVIDER_REFUSED))?;
    let separator = if flow.scope_param == "user_scope" {
        ","
    } else {
        " "
    };
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", &flow.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair(flow.scope_param, &flow.scopes.join(separator))
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        for (key, value) in &flow.extra {
            query.append_pair(key, value);
        }
    }
    Ok(url.into())
}

/// C.7: the code exchange; no client secret except Google's non-confidential one.
async fn exchange_code(
    inner: &Inner,
    flow: &Flow,
    code_in: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<Tokens, CredError> {
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code_in),
        ("code_verifier", verifier),
        ("redirect_uri", redirect_uri),
        ("client_id", &flow.client_id),
    ];
    if let Some(secret) = &flow.client_secret {
        form.push(("client_secret", secret));
    }
    let reply = inner
        .http
        .post_form(&flow.token, &form, None, inner.http.timeout)
        .await?;
    oauth::exchanged(oauth::read_tokens(reply)?)
}

/// C.6: polls the token endpoint every `interval` seconds until the person approves, denies, or the
/// code expires.
async fn poll_device(
    inner: &Inner,
    flow: &Flow,
    grant: &oauth::DeviceGrant,
) -> Result<Tokens, CredError> {
    let mut interval = grant.interval;
    loop {
        tokio::time::sleep(interval).await;
        if grant.expires_at < Timestamp::now() {
            return fail(code::SIGN_IN_TIMEOUT);
        }
        let form = [
            ("grant_type", DEVICE_GRANT_TYPE),
            ("device_code", grant.device_code.as_str()),
            ("client_id", flow.client_id.as_str()),
        ];
        let reply = inner
            .http
            .post_form(&flow.token, &form, None, inner.http.timeout)
            .await?;
        match oauth::read_tokens(reply)? {
            Outcome::Tokens(tokens) => return Ok(tokens),
            Outcome::Refused { error, .. } => match error.as_str() {
                "authorization_pending" => {}
                "slow_down" => interval += Duration::from_secs(5),
                "access_denied" => return fail(code::SIGN_IN_CANCELLED),
                "expired_token" => return fail(code::SIGN_IN_TIMEOUT),
                _ => return fail(code::PROVIDER_REFUSED),
            },
        }
    }
}

/// C.8: the account label and display name of a new sign-in.
async fn label_of(
    inner: &Inner,
    flow: &Flow,
    tokens: &Tokens,
) -> Result<(String, Option<String>), CredError> {
    let token = tokens.access_token.as_str();
    let claims = tokens
        .id_token
        .as_deref()
        .and_then(|t| oauth::id_token_claims(t));
    let read = |reply: oauth::Reply| -> Result<Value, CredError> {
        match reply.status {
            200..=299 => Ok(reply.body),
            500.. => fail(code::PROVIDER_UNREACHABLE),
            _ => fail(code::PROVIDER_REFUSED),
        }
    };
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    let cut = |name: Option<String>| -> Option<String> {
        name.filter(|n| !n.is_empty())
            .map(|n| n.chars().take(254).collect())
    };
    match &flow.provider {
        ProviderConfig::Oidc { .. }
        | ProviderConfig::Microsoft { .. }
        | ProviderConfig::Google { .. } => {
            let claims = claims.ok_or_else(|| CredError::of(code::PROVIDER_REFUSED))?;
            let label = oauth::id_token_label(&claims)
                .ok_or_else(|| CredError::of(code::PROVIDER_REFUSED))?;
            Ok((label, oauth::id_token_name(&claims)))
        }
        ProviderConfig::MailServerSso { mail_account, .. } => Ok((
            mail_account.clone(),
            claims.as_ref().and_then(oauth::id_token_name),
        )),
        ProviderConfig::Github { host } => {
            let api = if host == "github.com" {
                "https://api.github.com/user".to_owned()
            } else {
                format!("https://api.{host}/user")
            };
            let user = read(inner.http.get(&api, Auth::Bearer(token)).await?)?;
            let login =
                text(&user, "login").ok_or_else(|| CredError::of(code::PROVIDER_REFUSED))?;
            Ok((login, cut(text(&user, "name"))))
        }
        ProviderConfig::Gitlab { host, .. } => {
            let url = format!("https://{host}/api/v4/user");
            let user = read(inner.http.get(&url, Auth::Bearer(token)).await?)?;
            let name =
                text(&user, "username").ok_or_else(|| CredError::of(code::PROVIDER_REFUSED))?;
            Ok((name, cut(text(&user, "name"))))
        }
        ProviderConfig::Slack { .. } => match (&tokens.slack_team, &tokens.slack_user) {
            (Some(team), Some(user)) => Ok((format!("{team}/{user}"), None)),
            _ => fail(code::PROVIDER_REFUSED),
        },
        ProviderConfig::Confluence { .. } | ProviderConfig::Jira { .. } => {
            let site = flow
                .site
                .as_ref()
                .ok_or_else(|| CredError::of(code::PROVIDER_REFUSED))?;
            let url = "https://api.atlassian.com/oauth/token/accessible-resources";
            let resources = read(inner.http.get(url, Auth::Bearer(token)).await?)?;
            let wanted = site.as_str().trim_end_matches('/');
            let jira = flow.kind == CredentialKind::Jira;
            let found = resources.as_array().into_iter().flatten().any(|r| {
                let same = text(r, "url").is_some_and(|u| u.trim_end_matches('/') == wanted);
                let scoped = !jira
                    || r.get("scopes")
                        .and_then(Value::as_array)
                        .is_some_and(|s| s.iter().any(|v| v.as_str() == Some("read:jira-work")));
                same && scoped
            });
            if !found {
                return fail(code::PROVIDER_REFUSED);
            }
            Ok((site_host(site), None))
        }
    }
}

/// C.8–C.11: label, persist, publish, log.
async fn finish(
    inner: &Inner,
    flow: &Flow,
    tokens: Tokens,
    method: &str,
) -> Result<Credential, CredError> {
    let (label, display_name) = label_of(inner, flow, &tokens).await?;
    let chars = label.chars().count();
    if chars == 0 || chars > 254 {
        return fail(code::LABEL_INVALID);
    }
    if flow.kind == CredentialKind::Jira {
        let other = inner.active().into_iter().any(|r| {
            r.credential.kind == CredentialKind::Jira && r.credential.account_label != label
        });
        if other {
            return fail(code::DUPLICATE);
        }
    }
    let scopes = tokens.scopes.clone().unwrap_or_else(|| flow.scopes.clone());
    let blob = TokenBlob {
        access_token: tokens.access_token.to_string(),
        refresh_token: tokens.refresh_token.as_ref().map(|t| t.to_string()),
        expires_at: tokens.expires_at,
        scopes: scopes.clone(),
        token_type: tokens.token_type.clone(),
        method: (flow.kind == CredentialKind::Jira).then(|| "oauth".to_owned()),
        email: None,
        account_id: None,
        client_id: Some(flow.client_id.clone()),
        token_endpoint: Some(flow.token.clone()),
        client_secret: flow.client_secret.clone(),
    };
    let meta = Meta {
        provider_host: flow.provider_host.clone(),
        scopes,
        needs_sign_in: false,
        refused_at: None,
        last_error_code: None,
        display_name,
    };
    let now = Timestamp::now();
    let (row, is_new, previous) = match &flow.existing {
        Some(existing) => {
            let mut row = existing.clone();
            row.credential.account_label = label;
            row.credential.expires_at = tokens.expires_at;
            row.credential.refreshed_at = Some(now);
            row.meta = Meta {
                last_error_code: existing.meta.last_error_code.clone(),
                ..meta
            };
            (row, false, None)
        }
        None => (
            new_row(
                CredentialId::new(),
                flow.kind,
                label,
                now,
                tokens.expires_at,
                meta,
            ),
            true,
            None,
        ),
    };
    let bytes = Zeroizing::new(serde_json::to_vec(&blob).or_else(|_| fail(code::STORE_FAILED))?);
    let credential = inner.persist(row, is_new, &bytes, previous).await?;
    tracing::info!(
        target: "ordin_identity",
        challenge_id = flow.challenge_id.0.to_string().as_str(),
        credential_id = credential.id.to_string().as_str(),
        kind = kind_str(credential.kind),
        account_hash = short_hash(&credential.account_label).as_str(),
        method = method,
        "cred.sign_in.completed"
    );
    Ok(credential)
}
