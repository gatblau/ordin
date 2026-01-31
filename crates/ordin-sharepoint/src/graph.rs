//! Calling Graph: one request with the person's token, the timeouts, the status mapping, the retries
//! with back-off and jitter, and the paging links (SharePointAdapter.md Internal Logic 1).

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use ordin_identity::IdentityService;
use ordin_types::domain::identity::CredentialKind;
use reqwest::header::HeaderMap;
use reqwest::Method;
use tokio::sync::Semaphore;
use url::Url;

use crate::error::{from_cred, SharePointError};

pub(crate) const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";
pub(crate) const METADATA_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const TRANSFER_TIMEOUT: Duration = Duration::from_secs(120);
/// Retries after the first attempt.
pub(crate) const RETRIES: u32 = 3;
/// 1 s, 2 s, 4 s, each with ±20 % jitter; `Retry-After` wins when present.
const BACKOFF_SECONDS: [f64; 3] = [1.0, 2.0, 4.0];
/// *Assumed here*: the longest `Retry-After` this adapter waits for, so a hostile header cannot park a call.
const MAX_WAIT_SECONDS: u64 = 120;
/// At most 4 concurrent Graph requests per adapter (Performance).
const MAX_CONCURRENT: usize = 4;

/// The delegated scopes requested from Identity (*chosen here*).
pub const SCOPES: [&str; 2] = [
    "https://graph.microsoft.com/Files.ReadWrite.All",
    "https://graph.microsoft.com/Sites.Read.All",
];

/// What a failed send is: a mapped error, or a connection that was lost after the request left (a write's
/// outcome is then unknown, Internal Logic 6.6).
#[derive(Debug)]
pub(crate) enum Failure {
    Error(SharePointError),
    Lost,
}

impl From<SharePointError> for Failure {
    fn from(error: SharePointError) -> Self {
        Failure::Error(error)
    }
}

/// One Graph request.
pub(crate) struct Req {
    pub op: &'static str,
    method: Method,
    url: String,
    headers: Vec<(&'static str, String)>,
    body: Option<Bytes>,
    timeout: Option<Duration>,
    transfer: bool,
    write: bool,
    authenticated: bool,
    pass: &'static [u16],
}

impl Req {
    pub fn new(op: &'static str, method: Method, url: String) -> Req {
        Req {
            op,
            method,
            url,
            headers: Vec::new(),
            body: None,
            timeout: None,
            transfer: false,
            write: false,
            authenticated: true,
            pass: &[],
        }
    }

    pub fn get(op: &'static str, url: String) -> Req {
        Req::new(op, Method::GET, url)
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

    /// Uses `TRANSFER_TIMEOUT`.
    pub fn transfer(mut self) -> Req {
        self.transfer = true;
        self
    }

    /// A request that changes SharePoint: a lost connection is not retried.
    pub fn write(mut self) -> Req {
        self.write = true;
        self
    }

    /// Sent without the person's token (an upload session's own address).
    pub fn anonymous(mut self) -> Req {
        self.authenticated = false;
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
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Resp {
    /// A body that does not parse is `SP_RESPONSE_INVALID`.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, SharePointError> {
        serde_json::from_slice(&self.body).map_err(|_| SharePointError::ResponseInvalid)
    }
}

pub(crate) struct Graph {
    http: reqwest::Client,
    identity: Arc<IdentityService>,
    base: Url,
    /// Multiplies every wait and the transfer timeout; 1.0 outside the stand-in seam.
    wait_scale: f64,
    transfer_timeout: Duration,
    metadata_timeout: Duration,
    permits: Arc<Semaphore>,
}

impl Graph {
    pub fn new(
        identity: Arc<IdentityService>,
        http: reqwest::Client,
    ) -> Result<Graph, SharePointError> {
        Ok(Graph {
            http,
            identity,
            base: Url::parse(GRAPH_BASE).map_err(|_| SharePointError::ResponseInvalid)?,
            wait_scale: 1.0,
            transfer_timeout: TRANSFER_TIMEOUT,
            metadata_timeout: METADATA_TIMEOUT,
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT)),
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn use_stand_in(
        &mut self,
        base: &str,
        wait_scale: f64,
        transfer_timeout: Duration,
    ) -> Result<(), SharePointError> {
        self.base = Url::parse(base).map_err(|_| SharePointError::LocationInvalid)?;
        self.wait_scale = wait_scale.max(0.0);
        self.transfer_timeout = transfer_timeout;
        self.metadata_timeout = transfer_timeout.min(METADATA_TIMEOUT);
        Ok(())
    }

    /// `<base>/<segments…>?<query>`; each segment and value is percent-encoded by `url`.
    pub fn url(&self, segments: &[&str], query: &[(&str, &str)]) -> String {
        let mut url = self.base.clone();
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
        url.to_string()
    }

    /// A link Graph returned (`@odata.nextLink`, `@odata.deltaLink`) is followed only when it points at the
    /// same origin as the Graph base, so the token never goes anywhere else.
    pub fn follow(&self, link: &str) -> Result<String, SharePointError> {
        let url = Url::parse(link).map_err(|_| SharePointError::ResponseInvalid)?;
        let same = url.scheme() == self.base.scheme()
            && url.host_str() == self.base.host_str()
            && url.port_or_known_default() == self.base.port_or_known_default();
        if same {
            Ok(url.to_string())
        } else {
            Err(SharePointError::ResponseInvalid)
        }
    }

    /// Internal Logic 1.1.
    // spec: ordin-desktop/components/SharePointAdapter#SA-RECON-01
    async fn token(&self) -> Result<ordin_identity::AccessToken, SharePointError> {
        self.identity
            .access_token_for(CredentialKind::Microsoft, &SCOPES)
            .await
            .map_err(|e| from_cred(&e))
    }

    /// The scaled wait before the next attempt: `Retry-After` when Graph sent it, else the back-off step.
    fn wait_for(&self, attempt: u32, retry_after: Option<u64>) -> (Duration, u32) {
        let seconds = match retry_after {
            Some(s) => s.min(MAX_WAIT_SECONDS) as f64,
            None => {
                let step = BACKOFF_SECONDS[(attempt as usize).min(BACKOFF_SECONDS.len() - 1)];
                // ±20 % jitter from a random UUID's first byte.
                let random = f64::from(uuid::Uuid::new_v4().as_bytes()[0]) / 255.0;
                step * (0.8 + 0.4 * random)
            }
        };
        (
            Duration::from_secs_f64(seconds * self.wait_scale),
            seconds.round() as u32,
        )
    }

    /// Sends one request; see Internal Logic 1.2 and 1.3.
    pub async fn send(&self, req: Req) -> Result<Resp, Failure> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| SharePointError::Unavailable)?;
        let timeout = req.timeout.unwrap_or(if req.transfer {
            self.transfer_timeout
        } else {
            self.metadata_timeout
        });
        let mut attempt: u32 = 0;
        let mut refreshed = false;
        loop {
            let token = if req.authenticated {
                Some(self.token().await?)
            } else {
                None
            };
            let mut builder = self
                .http
                .request(req.method.clone(), req.url.as_str())
                .timeout(timeout);
            for (name, value) in &req.headers {
                builder = builder.header(*name, value.as_str());
            }
            if let Some(token) = &token {
                builder = builder.bearer_auth(token.token.expose());
            }
            if let Some(body) = &req.body {
                builder = builder.body(body.clone());
            }
            let started = Instant::now();
            let sent = match builder.send().await {
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
            };
            let elapsed_ms = started.elapsed().as_millis() as u64;
            let status = sent.as_ref().map_or(0, |r| r.status);
            tracing::info!(
                component = "sharepoint",
                operation = req.op,
                status = u64::from(status),
                elapsed_ms = elapsed_ms,
                attempt = u64::from(attempt + 1),
                "graph_request"
            );
            let resp = match sent {
                Ok(resp) => resp,
                Err(()) => {
                    // Reads retry; a write's request may have landed.
                    if req.write {
                        return Err(Failure::Lost);
                    }
                    if attempt >= RETRIES {
                        return Err(SharePointError::Unavailable.into());
                    }
                    let (wait, _) = self.wait_for(attempt, None);
                    attempt += 1;
                    tokio::time::sleep(wait).await;
                    continue;
                }
            };
            if (200..300).contains(&resp.status) || req.pass.contains(&resp.status) {
                return Ok(resp);
            }
            match resp.status {
                401 if req.authenticated => {
                    if refreshed {
                        return Err(SharePointError::SignInNeeded.into());
                    }
                    refreshed = true;
                    match self
                        .identity
                        .refresh_silently(CredentialKind::Microsoft)
                        .await
                    {
                        Ok(true) => continue,
                        Ok(false) => return Err(SharePointError::SignInNeeded.into()),
                        Err(e) => return Err(from_cred(&e).into()),
                    }
                }
                401 => return Err(SharePointError::SignInNeeded.into()),
                403 => return Err(SharePointError::AccessDenied.into()),
                404 => return Err(SharePointError::NotFound.into()),
                409 => return Err(SharePointError::NameExists.into()),
                413 => return Err(SharePointError::TooLarge.into()),
                429 | 503 => {
                    let retry_after = resp
                        .headers
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.trim().parse::<u64>().ok());
                    let (wait, seconds) = self.wait_for(attempt, retry_after);
                    if attempt >= RETRIES {
                        return Err(if resp.status == 429 {
                            SharePointError::Throttled {
                                retry_after_s: seconds,
                            }
                        } else {
                            SharePointError::Unavailable
                        }
                        .into());
                    }
                    tracing::warn!(
                        component = "sharepoint",
                        operation = req.op,
                        retry_after_s = u64::from(seconds),
                        "graph_throttled"
                    );
                    attempt += 1;
                    tokio::time::sleep(wait).await;
                }
                500..=599 => {
                    if req.write {
                        return Err(Failure::Lost);
                    }
                    if attempt >= RETRIES {
                        return Err(SharePointError::Unavailable.into());
                    }
                    let (wait, _) = self.wait_for(attempt, None);
                    attempt += 1;
                    tokio::time::sleep(wait).await;
                }
                _ => return Err(SharePointError::ResponseInvalid.into()),
            }
        }
    }

    /// As `send`, with a lost connection reported as `SP_UNAVAILABLE` (reads, and writes that are not
    /// checked afterwards).
    pub async fn call(&self, req: Req) -> Result<Resp, SharePointError> {
        match self.send(req).await {
            Ok(resp) => Ok(resp),
            Err(Failure::Error(e)) => Err(e),
            Err(Failure::Lost) => Err(SharePointError::Unavailable),
        }
    }
}
