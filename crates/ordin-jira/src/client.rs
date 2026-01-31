//! Calling Jira: credentials, base and header, status mapping, back-off, the concurrency limit and
//! the response size limit (JiraAdapter.md Internal Logic 1; RateLimiting cross-cutting policy).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ordin_identity::{CredError, IdentityService, JiraAccess, JiraCredentialMethod, SecretString};
use ordin_types::domain::identity::CredentialKind;
use ordin_types::CodedError as _;
use ordin_types::CredentialId;
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION, LOCATION, RETRY_AFTER};
use reqwest::Method;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt as _;

use crate::{JiraEndpoints, JiraError};

/// 3 attempts: the first and two repeats (`RETRIES`).
const ATTEMPTS: u32 = 3;
/// A `Retry-After` wins when it is at most this many seconds.
const MAX_RETRY_AFTER_SECS: u64 = 60;
/// `MAX_CONCURRENT_REQUESTS`.
const MAX_CONCURRENT_REQUESTS: usize = 4;
/// `MAX_RESPONSE_BYTES`: 10 MiB per JSON response.
pub(crate) const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;
/// `MAX_ATTACHMENT_BYTES`: 100 MiB.
pub(crate) const MAX_ATTACHMENT_BYTES: u64 = 100 * 1024 * 1024;
/// 120 s for an attachment download.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// A redirect chain on an attachment download is cut after this many hops.
const MAX_REDIRECTS: u32 = 5;

/// What the adapter needs of the Identity Service. The Identity Service implements it; the unit
/// tests stand in for it with a double, because a real service cannot be made to refuse on cue.
#[async_trait]
pub(crate) trait Credentials: Send + Sync {
    async fn jira_access(&self) -> Result<Option<JiraAccess>, CredError>;
    async fn refresh_silently(&self) -> Result<bool, CredError>;
    async fn report_refused(
        &self,
        credential_id: CredentialId,
        reason_code: &str,
    ) -> Result<(), CredError>;
    async fn save_jira_token(
        &self,
        site: &str,
        email: &str,
        token: SecretString,
        account_id: &str,
        display_name: &str,
    ) -> Result<(), CredError>;
}

#[async_trait]
impl Credentials for IdentityService {
    async fn jira_access(&self) -> Result<Option<JiraAccess>, CredError> {
        IdentityService::jira_access(self).await
    }
    async fn refresh_silently(&self) -> Result<bool, CredError> {
        IdentityService::refresh_silently(self, CredentialKind::Jira).await
    }
    async fn report_refused(
        &self,
        credential_id: CredentialId,
        reason_code: &str,
    ) -> Result<(), CredError> {
        IdentityService::report_refused(self, credential_id, reason_code).await
    }
    async fn save_jira_token(
        &self,
        site: &str,
        email: &str,
        token: SecretString,
        account_id: &str,
        display_name: &str,
    ) -> Result<(), CredError> {
        IdentityService::save_jira_token(self, site, email, token, account_id, display_name)
            .await
            .map(|_| ())
    }
}

/// Internal Logic 1.1: a `CredError` by its code.
pub(crate) fn map_cred(error: &CredError) -> JiraError {
    match error.code() {
        "CRED_SIGN_IN_REQUIRED"
        | "CRED_SCOPE_MISSING"
        | "CRED_PROVIDER_REFUSED"
        | "CRED_KEYCHAIN_MISSING" => JiraError::SignInNeeded,
        _ if error.retryable() => JiraError::Unavailable,
        _ => JiraError::SignInNeeded,
    }
}

/// How a write is treated when its outcome is unknown (Internal Logic 7.5).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Write {
    /// Idempotent: repeated once with the same body (`add_label`, `assign`).
    Repeat,
    /// Never resent here (`create_issue`, `add_comment`).
    NoResend,
}

/// One request, described once and sent up to 3 times.
pub(crate) struct Req {
    /// The log field `operation`.
    pub operation: &'static str,
    pub method: Method,
    pub segments: Vec<String>,
    pub query: Vec<(&'static str, String)>,
    pub body: Option<Value>,
    /// `None` for a read.
    pub write: Option<Write>,
    /// An attachment: streamed to this `.part` path.
    pub download: Option<PathBuf>,
}

impl Req {
    pub(crate) fn get(operation: &'static str, segments: &[&str]) -> Req {
        Req {
            operation,
            method: Method::GET,
            segments: segments.iter().map(|s| (*s).to_owned()).collect(),
            query: Vec::new(),
            body: None,
            write: None,
            download: None,
        }
    }

    pub(crate) fn query(mut self, name: &'static str, value: impl Into<String>) -> Req {
        self.query.push((name, value.into()));
        self
    }

    pub(crate) fn with_body(mut self, method: Method, body: Value) -> Req {
        self.method = method;
        self.body = Some(body);
        self
    }

    pub(crate) fn write(mut self, policy: Write) -> Req {
        self.write = Some(policy);
        self
    }
}

/// A 2xx answer.
pub(crate) struct Resp {
    pub body: Vec<u8>,
    /// Bytes written and SHA-256 hex, for a download.
    pub downloaded: Option<(u64, String)>,
}

impl Resp {
    pub(crate) fn json(&self) -> Result<Value, JiraError> {
        serde_json::from_slice(&self.body).map_err(|_| JiraError::ResponseInvalid)
    }
}

/// Where and how one call is addressed (Internal Logic 1.3).
pub(crate) struct Ctx {
    /// The URL the request segments are appended to.
    pub base: String,
    pub auth: HeaderValue,
    pub credential_id: Option<CredentialId>,
    pub oauth: bool,
}

/// Why an exchange failed. `Unauthorized` is handled by the caller (refresh or report).
pub(crate) enum Fail {
    Unauthorized,
    Jira(JiraError),
}

impl From<JiraError> for Fail {
    fn from(e: JiraError) -> Self {
        Fail::Jira(e)
    }
}

/// What one send produced, before the status is mapped.
enum Sent {
    Answer {
        status: u16,
        retry_after: Option<u64>,
        resp: Resp,
    },
    /// Nothing left this computer.
    ConnectFailed,
    /// The request may have reached Jira (timeout, reset after sending).
    Unknown,
    Failed(JiraError),
}

pub(crate) struct Transport {
    pub credentials: Arc<dyn Credentials>,
    pub http: reqwest::Client,
    /// Follows no redirect: attachment downloads follow them by hand (Internal Logic 4.6).
    pub plain: reqwest::Client,
    pub endpoints: JiraEndpoints,
    gate: tokio::sync::Semaphore,
    cloud_ids: Mutex<HashMap<CredentialId, String>>,
}

pub(crate) fn basic(email: &str, token: &str) -> HeaderValue {
    let mut value = HeaderValue::from_str(&format!(
        "Basic {}",
        STANDARD.encode(format!("{email}:{token}"))
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("Basic"));
    value.set_sensitive(true);
    value
}

fn bearer(token: &str) -> HeaderValue {
    let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
        .unwrap_or_else(|_| HeaderValue::from_static("Bearer"));
    value.set_sensitive(true);
    value
}

/// 1 s, 2 s, 4 s +/- 20 %; a `Retry-After` of at most 60 s wins.
fn delay(retries_done: u32, retry_after: Option<u64>) -> Duration {
    if let Some(secs) = retry_after.filter(|s| *s <= MAX_RETRY_AFTER_SECS) {
        return Duration::from_secs(secs);
    }
    let base_ms = 1000u64 << (retries_done.saturating_sub(1)).min(2);
    let byte = uuid::Uuid::new_v4().as_bytes()[0] as u64;
    // 0.8 .. 1.2 of the base, in steps of 1/255 of 40 %.
    let permille = 800 + byte * 400 / 255;
    Duration::from_millis(base_ms * permille / 1000)
}

/// An attachment redirect goes only to an `https` host ending `.atlassian.net`, `api.atlassian.com`
/// or `api.media.atlassian.com`.
pub(crate) fn redirect_allowed(url: &url::Url) -> bool {
    if url.scheme() != "https" || url.port().is_some() || !url.username().is_empty() {
        return false;
    }
    match url.host_str().map(str::to_ascii_lowercase) {
        Some(host) => {
            host.ends_with(".atlassian.net")
                || host == "api.atlassian.com"
                || host == "api.media.atlassian.com"
        }
        None => false,
    }
}

fn build_url(
    base: &str,
    segments: &[String],
    query: &[(&'static str, String)],
) -> Result<url::Url, JiraError> {
    let mut url = url::Url::parse(base).map_err(|_| JiraError::ResponseInvalid)?;
    url.path_segments_mut()
        .map_err(|_| JiraError::ResponseInvalid)?
        .pop_if_empty()
        .extend(segments);
    if !query.is_empty() {
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
    }
    Ok(url)
}

impl Transport {
    pub(crate) fn new(
        credentials: Arc<dyn Credentials>,
        http: reqwest::Client,
        endpoints: JiraEndpoints,
    ) -> Transport {
        let plain = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| http.clone());
        Transport {
            credentials,
            http,
            plain,
            endpoints,
            gate: tokio::sync::Semaphore::new(MAX_CONCURRENT_REQUESTS),
            cloud_ids: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn site_base(&self, site: &str) -> String {
        self.endpoints
            .site_base_url
            .clone()
            .unwrap_or_else(|| format!("https://{site}"))
    }

    fn gateway_base(&self) -> String {
        self.endpoints
            .gateway_base_url
            .clone()
            .unwrap_or_else(|| "https://api.atlassian.com".to_owned())
    }

    fn timeout(&self) -> Duration {
        self.endpoints
            .request_timeout
            .unwrap_or(crate::JiraAdapter::REQUEST_TIMEOUT)
    }

    fn forget_cloud_id(&self, id: CredentialId) {
        if let Ok(mut map) = self.cloud_ids.lock() {
            map.remove(&id);
        }
    }

    /// Internal Logic 1.1 and 1.2: the credential, and the check that the argument's site is its site.
    // spec: ordin-desktop/components/JiraAdapter#SA-JIRA-03
    pub(crate) async fn access(&self, site: &str) -> Result<JiraAccess, JiraError> {
        let access = self
            .credentials
            .jira_access()
            .await
            .map_err(|e| map_cred(&e))?
            .ok_or(JiraError::NotConnected)?;
        if access.site != site {
            return Err(JiraError::SiteMismatch);
        }
        Ok(access)
    }

    /// Internal Logic 1.3: base and header.
    pub(crate) async fn context(&self, site: &str) -> Result<Ctx, JiraError> {
        let access = self.access(site).await?;
        let credential_id = access.token.credential_id;
        match &access.method {
            JiraCredentialMethod::ApiToken { email } => Ok(Ctx {
                base: format!("{}/rest/api/3", self.site_base(site)),
                auth: basic(email, access.token.token.expose()),
                credential_id: Some(credential_id),
                oauth: false,
            }),
            JiraCredentialMethod::Oauth => {
                let auth = bearer(access.token.token.expose());
                let cloud_id = self.cloud_id(site, credential_id, &auth).await?;
                Ok(Ctx {
                    base: format!("{}/ex/jira/{cloud_id}/rest/api/3", self.gateway_base()),
                    auth,
                    credential_id: Some(credential_id),
                    oauth: true,
                })
            }
        }
    }

    /// `cloudId` from `accessible-resources`, once per credential (Shared Context, *Jira Cloud surface*).
    async fn cloud_id(
        &self,
        site: &str,
        credential_id: CredentialId,
        auth: &HeaderValue,
    ) -> Result<String, JiraError> {
        if let Some(id) = self
            .cloud_ids
            .lock()
            .ok()
            .and_then(|m| m.get(&credential_id).cloned())
        {
            return Ok(id);
        }
        let ctx = Ctx {
            base: self.gateway_base(),
            auth: auth.clone(),
            credential_id: Some(credential_id),
            oauth: true,
        };
        let req = Req::get(
            "accessible_resources",
            &["oauth", "token", "accessible-resources"],
        );
        let resp = match self.exchange(&ctx, &req).await {
            Ok(resp) => resp,
            Err(Fail::Unauthorized) => return Err(JiraError::SignInNeeded),
            Err(Fail::Jira(JiraError::AccessDenied)) => return Err(JiraError::SignInNeeded),
            Err(Fail::Jira(e)) => return Err(e),
        };
        let wanted = format!("https://{site}");
        let found = resp
            .json()?
            .as_array()
            .ok_or(JiraError::ResponseInvalid)?
            .iter()
            .find(|r| {
                r.get("url").and_then(Value::as_str) == Some(wanted.as_str())
                    && r.get("scopes")
                        .and_then(Value::as_array)
                        .is_some_and(|s| s.iter().any(|x| x.as_str() == Some("read:jira-work")))
            })
            .and_then(|r| r.get("id").and_then(Value::as_str).map(str::to_owned))
            .ok_or(JiraError::SignInNeeded)?;
        if let Ok(mut map) = self.cloud_ids.lock() {
            map.insert(credential_id, found.clone());
        }
        Ok(found)
    }

    /// One call with the person's stored credential (Internal Logic 1).
    pub(crate) async fn call(&self, site: &str, req: &Req) -> Result<Resp, JiraError> {
        let mut refreshed = false;
        loop {
            let ctx = self.context(site).await?;
            match self.exchange(&ctx, req).await {
                Ok(resp) => return Ok(resp),
                Err(Fail::Jira(e)) => return Err(e),
                Err(Fail::Unauthorized) => self.on_unauthorized(&ctx, &mut refreshed).await?,
            }
        }
    }

    /// Internal Logic 1.4, 401: OAuth refreshes once and the caller repeats; an API token, or a
    /// second 401, marks the credential refused.
    async fn on_unauthorized(&self, ctx: &Ctx, refreshed: &mut bool) -> Result<(), JiraError> {
        if ctx.oauth && !*refreshed {
            *refreshed = true;
            if let Some(id) = ctx.credential_id {
                self.forget_cloud_id(id);
            }
            self.credentials
                .refresh_silently()
                .await
                .map_err(|e| map_cred(&e))?;
            return Ok(());
        }
        if let Some(id) = ctx.credential_id {
            self.forget_cloud_id(id);
            // The mark is best effort: the caller is told to sign in whatever Identity answers.
            let _ = self
                .credentials
                .report_refused(id, "JIRA_TOKEN_REFUSED")
                .await;
        }
        Err(JiraError::SignInNeeded)
    }

    /// Sends `req` with `ctx`: back-off on 429/503 and on failures before sending, at most 3
    /// attempts; the status mapping of Internal Logic 1.4; the write rules of Internal Logic 7.
    pub(crate) async fn exchange(&self, ctx: &Ctx, req: &Req) -> Result<Resp, Fail> {
        let url = build_url(&ctx.base, &req.segments, &req.query)?;
        let mut tries = 0u32;
        let mut attempt = 0u32;
        let mut repeated = false;
        loop {
            attempt += 1;
            let started = Instant::now();
            let sent = {
                let _permit = self
                    .gate
                    .acquire()
                    .await
                    .map_err(|_| JiraError::Unavailable)?;
                self.send_once(ctx, req, &url).await
            };
            let elapsed_ms = started.elapsed().as_millis() as u64;
            let (status, retry_after, resp) = match sent {
                Sent::Failed(e) => return Err(e.into()),
                Sent::Answer {
                    status,
                    retry_after,
                    resp,
                } => {
                    tracing::info!(
                        component = "jira",
                        operation = req.operation,
                        status,
                        elapsed_ms,
                        attempt,
                        "jira_request"
                    );
                    (status, retry_after, resp)
                }
                Sent::ConnectFailed => {
                    tracing::info!(
                        component = "jira",
                        operation = req.operation,
                        status = 0u16,
                        elapsed_ms,
                        attempt,
                        "jira_request"
                    );
                    self.retry_or(&mut tries, req, None, JiraError::Unavailable)
                        .await?;
                    continue;
                }
                Sent::Unknown => {
                    tracing::info!(
                        component = "jira",
                        operation = req.operation,
                        status = 0u16,
                        elapsed_ms,
                        attempt,
                        "jira_request"
                    );
                    match req.write {
                        None => {
                            self.retry_or(&mut tries, req, None, JiraError::Unavailable)
                                .await?;
                        }
                        Some(policy) => {
                            self.unknown_write(policy, &mut repeated, req)?;
                        }
                    }
                    continue;
                }
            };
            match status {
                200..=299 => return Ok(resp),
                401 => return Err(Fail::Unauthorized),
                403 => return Err(JiraError::AccessDenied.into()),
                404 => return Err(JiraError::NotFound.into()),
                413 => return Err(JiraError::TooLarge.into()),
                400 if req.operation == "search" => return Err(JiraError::QueryInvalid.into()),
                400 if req.operation == "assign" => return Err(JiraError::AccessDenied.into()),
                429 => {
                    self.retry_or(&mut tries, req, retry_after, JiraError::Throttled)
                        .await?;
                }
                503 => {
                    self.retry_or(&mut tries, req, retry_after, JiraError::Unavailable)
                        .await?;
                }
                // Another 5xx on a write may have been applied, so it takes the unknown-outcome path.
                500..=599 => match req.write {
                    None => {
                        self.retry_or(&mut tries, req, retry_after, JiraError::Unavailable)
                            .await?;
                    }
                    Some(policy) => self.unknown_write(policy, &mut repeated, req)?,
                },
                // Another 4xx is an answer ordin cannot use, so `JIRA_RESPONSE_INVALID`.
                _ => return Err(JiraError::ResponseInvalid.into()),
            }
        }
    }

    /// Waits and returns `Ok` when an attempt is left, else the final error.
    async fn retry_or(
        &self,
        tries: &mut u32,
        req: &Req,
        retry_after: Option<u64>,
        last: JiraError,
    ) -> Result<(), JiraError> {
        *tries += 1;
        if *tries >= ATTEMPTS {
            return Err(last);
        }
        let wait = delay(*tries, retry_after);
        tracing::warn!(
            component = "jira",
            operation = req.operation,
            retry_after_ms = wait.as_millis() as u64,
            "jira_throttled"
        );
        tokio::time::sleep(wait).await;
        Ok(())
    }

    /// Internal Logic 7.5: after a send whose outcome is unknown, an idempotent write is repeated
    /// once; any other is `JIRA_OUTCOME_UNKNOWN`.
    fn unknown_write(
        &self,
        policy: Write,
        repeated: &mut bool,
        req: &Req,
    ) -> Result<(), JiraError> {
        if policy == Write::Repeat && !*repeated {
            *repeated = true;
            return Ok(());
        }
        tracing::warn!(
            component = "jira",
            operation = req.operation,
            "jira_outcome_unknown"
        );
        Err(JiraError::OutcomeUnknown)
    }

    async fn send_once(&self, ctx: &Ctx, req: &Req, url: &url::Url) -> Sent {
        if let Some(part) = &req.download {
            return self.send_download(ctx, url, part).await;
        }
        let mut builder = self
            .http
            .request(req.method.clone(), url.clone())
            .header(AUTHORIZATION, ctx.auth.clone())
            .header(ACCEPT, "application/json")
            .timeout(self.timeout());
        if let Some(body) = &req.body {
            builder = builder.json(body);
        }
        let response = match builder.send().await {
            Ok(r) => r,
            Err(e) if e.is_connect() => return Sent::ConnectFailed,
            Err(_) => return Sent::Unknown,
        };
        let status = response.status().as_u16();
        let retry_after = retry_after_secs(&response);
        if !(200..300).contains(&status) {
            return Sent::Answer {
                status,
                retry_after,
                resp: Resp {
                    body: Vec::new(),
                    downloaded: None,
                },
            };
        }
        match read_limited(response).await {
            Ok(body) => Sent::Answer {
                status,
                retry_after,
                resp: Resp {
                    body,
                    downloaded: None,
                },
            },
            Err(BodyFail::TooBig) => Sent::Failed(JiraError::ResponseInvalid),
            Err(BodyFail::Net) => Sent::Unknown,
        }
    }

    /// Internal Logic 4.6: redirects by hand, the Authorization header only on the first hop, the
    /// body streamed to the part file while it is hashed.
    async fn send_download(&self, ctx: &Ctx, url: &url::Url, part: &std::path::Path) -> Sent {
        let mut current = url.clone();
        let mut hops = 0u32;
        let response = loop {
            let mut builder = self
                .plain
                .get(current.clone())
                .header(ACCEPT, "*/*")
                .timeout(DOWNLOAD_TIMEOUT);
            if hops == 0 {
                builder = builder.header(AUTHORIZATION, ctx.auth.clone());
            }
            let response = match builder.send().await {
                Ok(r) => r,
                Err(e) if e.is_connect() => return Sent::ConnectFailed,
                Err(_) => return Sent::Unknown,
            };
            if !response.status().is_redirection() {
                break response;
            }
            hops += 1;
            let next = response
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|loc| current.join(loc).ok());
            match next {
                Some(next) if hops <= MAX_REDIRECTS && redirect_allowed(&next) => current = next,
                _ => return Sent::Failed(JiraError::ResponseInvalid),
            }
        };
        let status = response.status().as_u16();
        let retry_after = retry_after_secs(&response);
        let empty = || Resp {
            body: Vec::new(),
            downloaded: None,
        };
        if !(200..300).contains(&status) {
            return Sent::Answer {
                status,
                retry_after,
                resp: empty(),
            };
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_ATTACHMENT_BYTES)
        {
            return Sent::Failed(JiraError::TooLarge);
        }
        match stream_to(response, part).await {
            Ok(done) => Sent::Answer {
                status,
                retry_after,
                resp: Resp {
                    body: Vec::new(),
                    downloaded: Some(done),
                },
            },
            Err(StreamFail::TooBig) => Sent::Failed(JiraError::TooLarge),
            Err(StreamFail::Disk) => Sent::Failed(JiraError::Unavailable),
            Err(StreamFail::Net) => Sent::Unknown,
        }
    }
}

fn retry_after_secs(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
}

enum BodyFail {
    TooBig,
    Net,
}

async fn read_limited(mut response: reqwest::Response) -> Result<Vec<u8>, BodyFail> {
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
    {
        return Err(BodyFail::TooBig);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| BodyFail::Net)? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(BodyFail::TooBig);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

enum StreamFail {
    TooBig,
    Disk,
    Net,
}

/// Writes the body to `part` and returns the byte count and the SHA-256 in lowercase hex.
async fn stream_to(
    mut response: reqwest::Response,
    part: &std::path::Path,
) -> Result<(u64, String), StreamFail> {
    let mut file = tokio::fs::File::create(part)
        .await
        .map_err(|_| StreamFail::Disk)?;
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(_) => return Err(StreamFail::Net),
        };
        written += chunk.len() as u64;
        if written > MAX_ATTACHMENT_BYTES {
            return Err(StreamFail::TooBig);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|_| StreamFail::Disk)?;
    }
    file.flush().await.map_err(|_| StreamFail::Disk)?;
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((written, hex))
}

/// The back-off for the tests, which cannot see the private `delay`.
#[cfg(test)]
pub(crate) fn delay_for_tests(retries_done: u32, retry_after: Option<u64>) -> Duration {
    delay(retries_done, retry_after)
}
