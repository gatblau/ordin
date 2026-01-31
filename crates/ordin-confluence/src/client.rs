//! Calling Confluence: the settings, the person's token, the `cloudId`, the timeouts, the status mapping,
//! the retries with back-off and jitter, and the paging links (ConfluenceAdapter.md Internal Logic 1).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::Bytes;
use ordin_identity::{AccessToken, IdentityService};
use ordin_types::domain::identity::CredentialKind;
use ordin_types::CredentialId;
use reqwest::header::HeaderMap;
use reqwest::Method;
use tokio::sync::{watch, Semaphore};
use url::Url;

use crate::error::{from_cred, ConfluenceError};
use crate::types::ConfluenceSettings;
use crate::wire::{AccessibleResource, WireUser};

const GATEWAY: &str = "https://api.atlassian.com";
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(120);
/// Retries after the first attempt.
const RETRIES: u32 = 3;
/// 1 s, 2 s, 4 s, each with ±20 % jitter; `Retry-After` wins when present.
const BACKOFF_SECONDS: [f64; 3] = [1.0, 2.0, 4.0];
/// *Assumed here*: the longest `Retry-After` this adapter waits for, so a hostile header cannot park a call.
const MAX_WAIT_SECONDS: u64 = 120;
/// At most 4 concurrent Confluence requests (Performance).
const MAX_CONCURRENT: usize = 4;

/// The scopes every call asks of Identity: the spec's list without `read:content.restriction:confluence`.
/// That scope's name waits on C-11, and when Atlassian refuses it the audience read takes the C-42
/// fallback, so a credential without it must keep working for everything else.
// spec: ordin-desktop/components/ConfluenceAdapter#C-11
/// The OAuth scopes requested from Identity (ConfluenceAdapter.md, *OAuth scopes requested from
/// Identity*); `identity.sign_in` for `confluence` sends the same set (IdentityAndCredentials.md ID-R1-1).
pub const SCOPES: [&str; 11] = [
    "read:space:confluence",
    "read:page:confluence",
    "write:page:confluence",
    "read:attachment:confluence",
    "write:attachment:confluence",
    "delete:page:confluence",
    "delete:attachment:confluence",
    "read:space.permission:confluence",
    "search:confluence",
    "read:confluence-user",
    "offline_access",
];
/// Asked only by the audience read of a page.
pub(crate) const RESTRICTION_SCOPE: [&str; 1] = ["read:content.restriction:confluence"];

/// What a failed send is.
#[derive(Debug)]
pub(crate) enum Failure {
    /// A mapped error.
    Error(ConfluenceError),
    /// A write whose request may have landed: a timeout, a lost connection or a 5xx (step 7.6).
    Lost,
    /// A write answered 429 or 503, with the error it comes to when it cannot be retried.
    Busy(ConfluenceError),
}

impl From<ConfluenceError> for Failure {
    fn from(error: ConfluenceError) -> Self {
        Failure::Error(error)
    }
}

enum Target {
    /// Relative to `https://api.atlassian.com/ex/confluence/{cloudId}`.
    Site(String),
    /// Relative to the gateway itself.
    Gateway(&'static str),
}

/// One Confluence request.
pub(crate) struct Req {
    op: &'static str,
    method: Method,
    target: Target,
    headers: Vec<(&'static str, String)>,
    body: Option<Bytes>,
    transfer: bool,
    write: bool,
    pass: &'static [u16],
}

impl Req {
    pub fn new(op: &'static str, method: Method, relative: String) -> Req {
        Req {
            op,
            method,
            target: Target::Site(relative),
            headers: Vec::new(),
            body: None,
            transfer: false,
            write: false,
            pass: &[],
        }
    }

    pub fn get(op: &'static str, relative: String) -> Req {
        Req::new(op, Method::GET, relative)
    }

    fn accessible_resources() -> Req {
        Req {
            target: Target::Gateway("/oauth/token/accessible-resources"),
            ..Req::get("accessible_resources", String::new())
        }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Req {
        self.headers.push((name, value.into()));
        self
    }

    pub fn json(mut self, value: &serde_json::Value) -> Req {
        self.body = Some(Bytes::from(value.to_string()));
        self.headers
            .push(("content-type", "application/json".to_owned()));
        self
    }

    pub fn bytes(mut self, body: Bytes) -> Req {
        self.body = Some(body);
        self
    }

    /// Uses the 120 s transfer timeout.
    pub fn transfer(mut self) -> Req {
        self.transfer = true;
        self
    }

    /// A request that changes Confluence: only a read of the item can prove it did not land.
    pub fn write(mut self) -> Req {
        self.write = true;
        self
    }

    /// Statuses returned to the caller instead of being mapped to an error.
    pub fn pass(mut self, statuses: &'static [u16]) -> Req {
        self.pass = statuses;
        self
    }
}

pub(crate) struct Resp {
    pub status: u16,
    headers: HeaderMap,
    pub body: Bytes,
}

impl Resp {
    /// A body that does not parse is `CONF_RESPONSE_INVALID`.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, ConfluenceError> {
        serde_json::from_slice(&self.body).map_err(|_| ConfluenceError::ResponseInvalid)
    }

    fn retry_after(&self) -> Option<u64> {
        self.headers
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
    }
}

/// The token a `cloudId` was resolved with, without holding the token.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CloudKey {
    site: String,
    credential: CredentialId,
    expires_at: Option<i64>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `<path>?<query>`; each segment and value is percent-encoded by `url`.
pub(crate) fn relative(segments: &[&str], query: &[(&str, &str)]) -> String {
    // A base that always parses, used only to borrow `url`'s encoders.
    let Ok(mut url) = Url::parse("https://confluence.invalid/") else {
        return String::new();
    };
    if let Ok(mut path) = url.path_segments_mut() {
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query {
            pairs.append_pair(key, value);
        }
    }
    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    }
}

/// `_links.next` is followed only as a path on the same gateway, so the token never goes anywhere else.
/// v2 links start `/wiki/`; v1 links are relative to `/wiki`.
pub(crate) fn next_relative(link: &str) -> Result<String, ConfluenceError> {
    if link.starts_with("//") || !link.starts_with('/') {
        return Err(ConfluenceError::ResponseInvalid);
    }
    if link.starts_with("/wiki/") {
        Ok(link.to_owned())
    } else {
        Ok(format!("/wiki{link}"))
    }
}

/// The connection settings a test puts in place of the production ones (CONF-SEAM-2): the gateway,
/// the company site and the token. Compiled only with the `test-support` feature.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone)]
pub struct ConfluenceTestSeam {
    /// `https://…`, or `http://` on `127.0.0.1` or `localhost`.
    pub gateway: String,
    /// Matched against `accessible-resources`, as the profile's site is; never called itself.
    pub site: Url,
    /// Sent as `Bearer`; Identity is not asked.
    pub token: String,
}

/// The seam's settings as the transport keeps them.
#[cfg(any(test, feature = "test-support"))]
struct Seam {
    gateway: String,
    site: String,
    token: String,
    credential: CredentialId,
}

/// CONF-SEAM-2: `https`, or plain `http` on a loopback host.
#[cfg(any(test, feature = "test-support"))]
fn gateway_allowed(address: &str) -> Option<String> {
    let parsed = Url::parse(address).ok()?;
    let loopback = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"));
    let allowed = parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback);
    (allowed && parsed.query().is_none() && parsed.fragment().is_none())
        .then(|| address.trim_end_matches('/').to_owned())
}

pub(crate) struct Transport {
    http: reqwest::Client,
    identity: Arc<IdentityService>,
    settings: watch::Receiver<ConfluenceSettings>,
    gateway: String,
    /// CONF-SEAM-2; `None` until a test puts its settings in.
    #[cfg(any(test, feature = "test-support"))]
    seam: Mutex<Option<Seam>>,
    /// Multiplies every wait; 1.0 outside the stand-in seam.
    wait_scale: f64,
    transfer_timeout: Duration,
    metadata_timeout: Duration,
    permits: Semaphore,
    cloud: Mutex<Option<(CloudKey, String)>>,
    /// Author names, per account for the session.
    authors: Mutex<HashMap<String, Option<String>>>,
    /// Replaces `https://{site}` for an API-token request (the stand-in seam); `None` in production.
    site_base: Option<String>,
    /// The site of the last API-token request, `https://{site}`, so a page's web address names the site
    /// the token reaches rather than the profile's.
    token_site: Mutex<Option<String>>,
}

/// How one request is signed (CONF-TOKEN-2).
pub(crate) enum Auth {
    /// A browser sign-in through Identity, sent to the gateway with the site's `cloudId`.
    Bearer(AccessToken),
    /// A pasted API token, sent straight to the site with Basic authentication as `email:token`.
    Basic {
        /// `https://{name}.atlassian.net`, without a trailing `/`.
        site: String,
        email: String,
        token: ordin_identity::SecretString,
        credential: CredentialId,
    },
}

impl Transport {
    pub fn new(
        identity: Arc<IdentityService>,
        http: reqwest::Client,
        settings: watch::Receiver<ConfluenceSettings>,
    ) -> Transport {
        Transport {
            http,
            identity,
            settings,
            gateway: GATEWAY.to_owned(),
            #[cfg(any(test, feature = "test-support"))]
            seam: Mutex::new(None),
            wait_scale: 1.0,
            transfer_timeout: TRANSFER_TIMEOUT,
            metadata_timeout: METADATA_TIMEOUT,
            permits: Semaphore::new(MAX_CONCURRENT),
            cloud: Mutex::new(None),
            authors: Mutex::new(HashMap::new()),
            site_base: None,
            token_site: Mutex::new(None),
        }
    }

    /// The address an API-token request for `host` goes to: `https://{host}`, or the stand-in's.
    pub fn site_base(&self, host: &str) -> String {
        self.site_base
            .clone()
            .unwrap_or_else(|| format!("https://{host}"))
    }

    /// Identity, which keeps a checked API token (CONF-TOKEN-1).
    pub fn identity(&self) -> &IdentityService {
        &self.identity
    }

    /// The HTTP client and the metadata timeout, for the credential check of `connect_with_token`.
    pub fn check_client(&self) -> (&reqwest::Client, Duration) {
        (&self.http, self.metadata_timeout)
    }

    /// How the next request is signed: the test seam's token, else a pasted API token when the
    /// person's Confluence credential is one, else the browser sign-in's token through Identity
    /// (CONF-TOKEN-2; amended January 2026; user decision; Approved by the user January 2026).
    // spec: ordin-desktop/components/ConfluenceAdapter#CONF-TOKEN-2
    async fn auth(&self) -> Result<Auth, ConfluenceError> {
        if !self.seamed() {
            let pasted = self
                .identity
                .confluence_token_access()
                .await
                .map_err(|e| from_cred(&e))?;
            if let Some(access) = pasted {
                let site = format!("https://{}", access.site);
                *lock(&self.token_site) = Some(site.clone());
                return Ok(Auth::Basic {
                    site,
                    email: access.email,
                    credential: access.token.credential_id,
                    token: access.token.token,
                });
            }
        }
        *lock(&self.token_site) = None;
        // As before the API token: with no site in the settings, nothing is asked of Identity.
        self.site()?;
        Ok(Auth::Bearer(self.token().await?))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn use_stand_in(&mut self, gateway: &str, wait_scale: f64, timeout: Duration) {
        gateway.trim_end_matches('/').clone_into(&mut self.gateway);
        self.site_base = Some(self.gateway.clone());
        self.wait_scale = wait_scale.max(0.0);
        self.transfer_timeout = timeout;
        self.metadata_timeout = timeout.min(METADATA_TIMEOUT);
    }

    /// CONF-SEAM-2: every later request goes to `seam.gateway`, for `seam.site`, with `seam.token`.
    /// An address that is neither `https` nor loopback `http` is `CONF_NOT_CONFIGURED`, and nothing
    /// changes.
    // spec: ordin-desktop/components/ConfluenceAdapter#CONF-SEAM-2
    #[cfg(any(test, feature = "test-support"))]
    pub fn use_test_seam(&self, seam: ConfluenceTestSeam) -> Result<(), ConfluenceError> {
        let gateway = gateway_allowed(&seam.gateway).ok_or(ConfluenceError::NotConfigured)?;
        *lock(&self.seam) = Some(Seam {
            gateway,
            site: seam.site.as_str().trim_end_matches('/').to_owned(),
            token: seam.token,
            credential: CredentialId::new(),
        });
        *lock(&self.cloud) = None;
        Ok(())
    }

    /// The gateway every request goes to: Atlassian's, or the seam's in a test (CONF-SEAM-1).
    // spec: ordin-desktop/components/ConfluenceAdapter#CONF-SEAM-1
    fn gateway(&self) -> String {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(seam) = lock(&self.seam).as_ref() {
            return seam.gateway.clone();
        }
        self.gateway.clone()
    }

    /// Whether a test's seam is in place.
    fn seamed(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        if lock(&self.seam).is_some() {
            return true;
        }
        false
    }

    /// The company site, without a trailing `/`. `None` in the settings is `CONF_NOT_CONFIGURED`.
    pub fn site(&self) -> Result<String, ConfluenceError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(seam) = lock(&self.seam).as_ref() {
            return Ok(seam.site.clone());
        }
        if let Some(site) = lock(&self.token_site).as_ref() {
            return Ok(site.clone());
        }
        let settings = self.settings.borrow();
        let site = settings
            .site
            .as_ref()
            .ok_or(ConfluenceError::NotConfigured)?;
        Ok(site.as_str().trim_end_matches('/').to_owned())
    }

    /// Internal Logic 1.1. However the sign-in works, the token arrives through Identity.
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-07
    // spec: ordin-desktop/components/ConfluenceAdapter#C-12
    async fn token(&self) -> Result<AccessToken, ConfluenceError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(seam) = lock(&self.seam).as_ref() {
            return Ok(AccessToken {
                token: ordin_identity::SecretString::new(seam.token.clone()),
                expires_at: None,
                credential_id: seam.credential,
            });
        }
        self.identity
            .access_token_for(CredentialKind::Confluence, &SCOPES)
            .await
            .map_err(|e| from_cred(&e))
    }

    /// Whether the signed-in credential holds the restriction-read scope (the audience is the space's
    /// when not, C-42).
    pub async fn holds_restriction_scope(&self) -> Result<bool, ConfluenceError> {
        if self.seamed() {
            return Ok(true);
        }
        // An API token acts with all of the person's own rights; it has no scopes to lack.
        if self
            .identity
            .confluence_token_access()
            .await
            .map_err(|e| from_cred(&e))?
            .is_some()
        {
            return Ok(true);
        }
        let mut scopes = SCOPES.to_vec();
        scopes.extend(RESTRICTION_SCOPE);
        match self
            .identity
            .access_token_for(CredentialKind::Confluence, &scopes)
            .await
        {
            Ok(_) => Ok(true),
            Err(e) if ordin_types::CodedError::code(&e) == "CRED_SCOPE_MISSING" => Ok(false),
            Err(e) => Err(from_cred(&e)),
        }
    }

    /// Internal Logic 1.2: cached per token, and a site change misses the cache.
    // spec: ordin-desktop/components/ConfluenceAdapter#CONF_SITE_NOT_GRANTED
    async fn cloud_id(&self, site: &str, token: &AccessToken) -> Result<String, Failure> {
        let key = CloudKey {
            site: site.to_owned(),
            credential: token.credential_id,
            expires_at: token.expires_at.map(|t| t.unix_millis()),
        };
        if let Some((held, id)) = lock(&self.cloud).as_ref() {
            if *held == key {
                return Ok(id.clone());
            }
        }
        let reply = Box::pin(self.send(Req::accessible_resources())).await?;
        let resources: Vec<AccessibleResource> = reply.json()?;
        let id = resources
            .into_iter()
            .find(|r| r.url.trim_end_matches('/') == site)
            .map(|r| r.id)
            .ok_or(ConfluenceError::SiteNotGranted)?;
        *lock(&self.cloud) = Some((key, id.clone()));
        Ok(id)
    }

    /// The scaled wait before the next attempt: `Retry-After` when Confluence sent it, else the back-off.
    fn wait_for(&self, attempt: u32, retry_after: Option<u64>) -> Duration {
        let seconds = match retry_after {
            Some(s) => s.min(MAX_WAIT_SECONDS) as f64,
            None => {
                let step = BACKOFF_SECONDS[(attempt as usize).min(BACKOFF_SECONDS.len() - 1)];
                // ±20 % jitter from a random UUID's first byte.
                let random = f64::from(uuid::Uuid::new_v4().as_bytes()[0]) / 255.0;
                step * (0.8 + 0.4 * random)
            }
        };
        Duration::from_secs_f64(seconds * self.wait_scale)
    }

    /// The wait before a write is retried (step 7.6).
    pub async fn pause(&self) {
        tokio::time::sleep(self.wait_for(0, None)).await;
    }

    /// Sends one request; see Internal Logic 1.3.
    pub async fn send(&self, req: Req) -> Result<Resp, Failure> {
        let timeout = if req.transfer {
            self.transfer_timeout
        } else {
            self.metadata_timeout
        };
        let mut attempt: u32 = 0;
        let mut refreshed = false;
        loop {
            let auth = self.auth().await?;
            let gateway = self.gateway();
            let (url, basic) = match (&auth, &req.target) {
                (Auth::Bearer(_), Target::Gateway(tail)) => (format!("{gateway}{tail}"), None),
                (Auth::Bearer(token), Target::Site(tail)) => {
                    let site = self.site()?;
                    let cloud = self.cloud_id(&site, token).await?;
                    (format!("{gateway}/ex/confluence/{cloud}{tail}"), None)
                }
                // CONF-TOKEN-2: an API token goes to the site itself; it has no gateway look-up.
                (
                    Auth::Basic {
                        site, credential, ..
                    },
                    target,
                ) => {
                    let host = site.trim_start_matches("https://");
                    let tail = match target {
                        Target::Site(tail) => tail.as_str(),
                        Target::Gateway(tail) => tail,
                    };
                    (format!("{}{tail}", self.site_base(host)), Some(*credential))
                }
            };
            let builder = self.http.request(req.method.clone(), url).timeout(timeout);
            let mut builder = match &auth {
                Auth::Bearer(token) => builder.bearer_auth(token.token.expose()),
                Auth::Basic { email, token, .. } => builder.basic_auth(email, Some(token.expose())),
            };
            for (name, value) in &req.headers {
                builder = builder.header(*name, value.as_str());
            }
            if let Some(body) = &req.body {
                builder = builder.body(body.clone());
            }
            let started = Instant::now();
            let sent = {
                // Held for the exchange only, so a wait never takes a place in the four.
                let _permit = self
                    .permits
                    .acquire()
                    .await
                    .map_err(|_| ConfluenceError::Unavailable)?;
                match builder.send().await {
                    Ok(response) => {
                        let status = response.status().as_u16();
                        let headers = response.headers().clone();
                        response
                            .bytes()
                            .await
                            .map(|body| Resp {
                                status,
                                headers,
                                body,
                            })
                            .map_err(|_| ())
                    }
                    Err(_) => Err(()),
                }
            };
            let elapsed_ms = started.elapsed().as_millis() as u64;
            let status = sent.as_ref().map_or(0, |r| r.status);
            tracing::info!(
                component = "confluence",
                operation = req.op,
                status = u64::from(status),
                elapsed_ms = elapsed_ms,
                attempt = u64::from(attempt + 1),
                "confluence_request"
            );
            let resp = match sent {
                Ok(resp) => resp,
                Err(()) => {
                    if req.write {
                        return Err(Failure::Lost);
                    }
                    if attempt >= RETRIES {
                        return Err(ConfluenceError::Unavailable.into());
                    }
                    let wait = self.wait_for(attempt, None);
                    attempt += 1;
                    tokio::time::sleep(wait).await;
                    continue;
                }
            };
            if (200..300).contains(&resp.status) || req.pass.contains(&resp.status) {
                return Ok(resp);
            }
            match resp.status {
                401 => {
                    // A pasted API token has nothing to refresh: it is marked as needing a new
                    // token, best effort, and the caller is told to sign in (CONF-TOKEN-2).
                    if let Some(credential) = basic {
                        let _ = self
                            .identity
                            .report_refused(credential, "CONF_SIGN_IN_NEEDED")
                            .await;
                        *lock(&self.token_site) = None;
                        return Err(ConfluenceError::SignInNeeded.into());
                    }
                    // A seam's token has no sign-in behind it to refresh (CONF-SEAM-2).
                    if refreshed || self.seamed() {
                        return Err(ConfluenceError::SignInNeeded.into());
                    }
                    refreshed = true;
                    match self
                        .identity
                        .refresh_silently(CredentialKind::Confluence)
                        .await
                    {
                        Ok(true) => continue,
                        Ok(false) => return Err(ConfluenceError::SignInNeeded.into()),
                        Err(e) => return Err(from_cred(&e).into()),
                    }
                }
                400 if says_title_exists(&resp.body) => {
                    return Err(ConfluenceError::TitleExists.into())
                }
                403 => return Err(ConfluenceError::AccessDenied.into()),
                404 => return Err(ConfluenceError::NotFound.into()),
                413 => return Err(ConfluenceError::TooLarge.into()),
                429 | 503 => {
                    let error = if resp.status == 429 {
                        ConfluenceError::Throttled
                    } else {
                        ConfluenceError::Unavailable
                    };
                    if req.write {
                        return Err(Failure::Busy(error));
                    }
                    if attempt >= RETRIES {
                        return Err(error.into());
                    }
                    let wait = self.wait_for(attempt, resp.retry_after());
                    attempt += 1;
                    tokio::time::sleep(wait).await;
                }
                500..=599 => {
                    if req.write {
                        return Err(Failure::Lost);
                    }
                    if attempt >= RETRIES {
                        return Err(ConfluenceError::Unavailable.into());
                    }
                    let wait = self.wait_for(attempt, None);
                    attempt += 1;
                    tokio::time::sleep(wait).await;
                }
                _ => return Err(ConfluenceError::ResponseInvalid.into()),
            }
        }
    }

    /// As `send`, for reads.
    pub async fn call(&self, req: Req) -> Result<Resp, ConfluenceError> {
        match self.send(req).await {
            Ok(resp) => Ok(resp),
            Err(Failure::Error(e) | Failure::Busy(e)) => Err(e),
            Err(Failure::Lost) => Err(ConfluenceError::Unavailable),
        }
    }

    /// The display name of an account, cached for the session. A failed look-up names nobody: an author's
    /// name never fails a read.
    pub async fn author(&self, account: Option<&str>) -> Option<String> {
        let account = account?;
        if let Some(name) = lock(&self.authors).get(account) {
            return name.clone();
        }
        let url = relative(&["wiki", "rest", "api", "user"], &[("accountId", account)]);
        let name = match self.call(Req::get("author_name", url)).await {
            Ok(reply) => reply
                .json::<WireUser>()
                .ok()
                .and_then(|user| user.display_name),
            Err(_) => return None,
        };
        lock(&self.authors).insert(account.to_owned(), name.clone());
        name
    }
}

/// Internal Logic 1.3: "400 whose body says a page with this title exists".
fn says_title_exists(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body).to_lowercase();
    text.contains("title") && text.contains("exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_and_values_are_percent_encoded() {
        assert_eq!(
            relative(&["wiki", "api", "v2", "pages", "1 2"], &[("q", "a b&c")]),
            "/wiki/api/v2/pages/1%202?q=a+b%26c"
        );
        assert_eq!(relative(&["wiki"], &[]), "/wiki");
    }

    #[test]
    fn a_next_link_stays_on_the_gateway() {
        assert_eq!(
            next_relative("/wiki/api/v2/pages/1/children?cursor=x").unwrap(),
            "/wiki/api/v2/pages/1/children?cursor=x"
        );
        assert_eq!(
            next_relative("/rest/api/search?cursor=x").unwrap(),
            "/wiki/rest/api/search?cursor=x"
        );
        for bad in [
            "https://elsewhere.example/wiki/x",
            "//elsewhere.example/x",
            "x",
        ] {
            assert_eq!(
                next_relative(bad).unwrap_err().code(),
                "CONF_RESPONSE_INVALID",
                "{bad}"
            );
        }
    }

    // spec: ordin-desktop/components/ConfluenceAdapter#CONF-SEAM-2
    #[test]
    fn a_test_seam_takes_https_or_loopback_http_only() {
        for good in [
            "https://gateway.example",
            "http://127.0.0.1:41234",
            "http://localhost:8080/",
        ] {
            assert!(gateway_allowed(good).is_some(), "{good}");
        }
        assert_eq!(
            gateway_allowed("http://127.0.0.1:41234/").as_deref(),
            Some("http://127.0.0.1:41234")
        );
        for bad in [
            "http://gateway.example",
            "http://10.0.0.1:80",
            "ftp://127.0.0.1",
            "https://gateway.example?x=1",
            "not an address",
        ] {
            assert!(gateway_allowed(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_title_clash_is_read_from_the_body() {
        assert!(says_title_exists(
            br#"{"message":"A page with this title already exists"}"#
        ));
        assert!(!says_title_exists(br#"{"message":"Bad request"}"#));
    }
}
