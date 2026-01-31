//! HTTP plumbing: one client per agent origin, kept for as long as the program runs so the open
//! connections are reused (Internal Logic A), the agent service token (SA-KQC-04), and the bounded
//! JSON reads every non-streaming route uses.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ordin_identity::SecretString;
use ordin_types::domain::agent_profile::KnowledgeProxy;
use reqwest::redirect::Policy;

use crate::types::{ServiceTokenSettings, IDLE_TIMEOUT_SECONDS};

/// The largest non-streaming answer read: a contents document is at most 2 MiB (Data Model).
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

pub(crate) struct Transport {
    fallback: reqwest::Client,
    clients: Mutex<HashMap<String, reqwest::Client>>,
    /// Test stand-in: maps the start of an address (`https://kfd.example.org`) to a loopback address.
    #[cfg(test)]
    pub(crate) rewrites: Mutex<HashMap<String, String>>,
}

impl Transport {
    pub(crate) fn new(fallback: reqwest::Client) -> Transport {
        Transport {
            fallback,
            clients: Mutex::new(HashMap::new()),
            #[cfg(test)]
            rewrites: Mutex::new(HashMap::new()),
        }
    }

    /// The client of the agent at `url`, created on first use (Internal Logic A): redirects refused,
    /// idle connections kept 90 s, at most 4 idle per host, TCP keep-alive 30 s.
    pub(crate) fn client(&self, url: &str) -> reqwest::Client {
        let origin = url::Url::parse(url)
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_else(|_| url.to_owned());
        let Ok(mut clients) = self.clients.lock() else {
            return self.fallback.clone();
        };
        clients
            .entry(origin)
            .or_insert_with(|| {
                reqwest::Client::builder()
                    .redirect(Policy::none())
                    .pool_idle_timeout(Duration::from_secs(IDLE_TIMEOUT_SECONDS))
                    .pool_max_idle_per_host(4)
                    .tcp_keepalive(Duration::from_secs(30))
                    .build()
                    .unwrap_or_else(|_| self.fallback.clone())
            })
            .clone()
    }

    /// The address a request is sent to. Production sends it as given.
    pub(crate) fn address(&self, url: String) -> String {
        #[cfg(test)]
        if let Ok(rewrites) = self.rewrites.lock() {
            for (from, to) in rewrites.iter() {
                if let Some(rest) = url.strip_prefix(from.as_str()) {
                    return format!("{to}{rest}");
                }
            }
        }
        url
    }

    /// Drops the per-agent clients, which closes their connections.
    pub(crate) fn close(&self) {
        if let Ok(mut clients) = self.clients.lock() {
            clients.clear();
        }
    }
}

/// What a bounded read of a plain answer yields.
pub(crate) struct Plain {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Sends `request` and reads at most `MAX_BODY_BYTES` of the answer. A network failure, a timeout
/// or an oversized answer is `Err(())`: callers map it to `KQ_UNREACHABLE` (or leave states as
/// they were).
pub(crate) async fn read_plain(request: reqwest::RequestBuilder) -> Result<Plain, ()> {
    let response = request.send().await.map_err(|_| ())?;
    let status = response.status().as_u16();
    if response
        .content_length()
        .is_some_and(|n| n as usize > MAX_BODY_BYTES)
    {
        return Err(());
    }
    let body = response.bytes().await.map_err(|_| ())?;
    if body.len() > MAX_BODY_BYTES {
        return Err(());
    }
    Ok(Plain {
        status,
        body: body.to_vec(),
    })
}

/// Why a service token could not be had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceTokenFailure {
    /// The identity provider refused the grant.
    Refused,
    /// The token endpoint could not be reached or answered nonsense.
    Unreachable,
}

/// The agent profile's OAuth 2.0 client-credentials grant, cached per proxy until 5 minutes before
/// it expires (Internal Logic H.1).
// spec: ordin-desktop/components/KnowledgeQueryClient#SA-KQC-04
pub(crate) struct ServiceTokens {
    settings: ServiceTokenSettings,
    cache: tokio::sync::Mutex<HashMap<String, (SecretString, Instant)>>,
}

impl ServiceTokens {
    pub(crate) fn new(settings: ServiceTokenSettings) -> ServiceTokens {
        ServiceTokens {
            settings,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn invalidate(&self, proxy: &str) {
        self.cache.lock().await.remove(proxy);
    }

    pub(crate) async fn token_for(
        &self,
        transport: &Transport,
        proxy: &KnowledgeProxy,
    ) -> Result<SecretString, ServiceTokenFailure> {
        let mut cache = self.cache.lock().await;
        if let Some((token, until)) = cache.get(&proxy.name) {
            if Instant::now() < *until {
                return Ok(SecretString::new(token.expose().to_owned()));
            }
        }
        let identity = &self.settings.identity;
        let form_body = {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("grant_type", "client_credentials");
            form.append_pair("client_id", &identity.client_id);
            form.append_pair("client_secret", self.settings.client_secret.expose());
            if let Some(scope) = identity.scope.as_deref() {
                form.append_pair("scope", scope);
            }
            form.append_pair("audience", &proxy.audience);
            form.finish()
        };
        let request = transport
            .client(&identity.token_url)
            .post(transport.address(identity.token_url.clone()))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .timeout(Duration::from_secs(crate::types::ABOUT_TIMEOUT_SECONDS))
            .body(form_body);
        let plain = read_plain(request)
            .await
            .map_err(|_| ServiceTokenFailure::Unreachable)?;
        if matches!(plain.status, 400 | 401 | 403) {
            return Err(ServiceTokenFailure::Refused);
        }
        if plain.status != 200 {
            return Err(ServiceTokenFailure::Unreachable);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&plain.body).map_err(|_| ServiceTokenFailure::Unreachable)?;
        let token = value
            .get("access_token")
            .and_then(|v| v.as_str())
            .filter(|t| !t.is_empty())
            .ok_or(ServiceTokenFailure::Unreachable)?
            .to_owned();
        let lifetime = value
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .saturating_sub(300);
        if lifetime > 0 {
            cache.insert(
                proxy.name.clone(),
                (
                    SecretString::new(token.clone()),
                    Instant::now() + Duration::from_secs(lifetime),
                ),
            );
        }
        Ok(SecretString::new(token))
    }
}
