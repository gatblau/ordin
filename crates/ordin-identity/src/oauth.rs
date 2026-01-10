//! The OAuth 2.0 grant shapes this crate runs itself over `reqwest` (Approach): authorisation code with
//! PKCE, the device authorisation grant (RFC 8628), the refresh grant, and the provider replies they read
//! (Internal Logic C.4–C.9, E.4, H.2).

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ordin_types::time::Timestamp;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::error::{code, fail, CredError};
use crate::secret::random_fill;

/// HTTP timeout per token-endpoint call (Fixed constants).
pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
/// Revocation call timeout on sign-out (G.2).
pub(crate) const REVOKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The PKCE verifier's alphabet, `[A-Za-z0-9-._~]`, and its length.
const VERIFIER_ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
const VERIFIER_CHARS: usize = 64;

/// The shared HTTP client, with the per-call timeout.
pub(crate) struct Http {
    pub client: reqwest::Client,
    pub timeout: Duration,
    /// Unit tests send every provider call to a loopback stand-in: `https://<host>/<path>` becomes
    /// `<redirect>/<host>/<path>`. Absent from every build but the test build.
    #[cfg(test)]
    pub redirect: Option<String>,
}

/// One provider reply: the status and the body as JSON (`Null` when the body is not JSON).
pub(crate) struct Reply {
    pub status: u16,
    pub body: Value,
}

impl Http {
    pub(crate) fn url(&self, url: &str) -> String {
        #[cfg(test)]
        if let Some(base) = &self.redirect {
            if let Some(rest) = url.strip_prefix("https://") {
                return format!("{}/{rest}", base.trim_end_matches('/'));
            }
        }
        url.to_owned()
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
        timeout: Duration,
    ) -> Result<Reply, CredError> {
        let unreachable = |_| CredError::of(code::PROVIDER_UNREACHABLE);
        let response = request
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(timeout)
            .send()
            .await
            .map_err(unreachable)?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(unreachable)?;
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok(Reply { status, body })
    }

    /// POST `application/x-www-form-urlencoded`, optionally with a bearer token.
    pub(crate) async fn post_form(
        &self,
        url: &str,
        form: &[(&str, &str)],
        bearer: Option<&str>,
        timeout: Duration,
    ) -> Result<Reply, CredError> {
        let body = Zeroizing::new(
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(form)
                .finish(),
        );
        let mut request = self
            .client
            .post(self.url(url))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body.as_str().to_owned());
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        self.send(request, timeout).await
    }

    /// GET with one optional header (`Authorization: Bearer` or GitLab's `PRIVATE-TOKEN`).
    pub(crate) async fn get(&self, url: &str, auth: Auth<'_>) -> Result<Reply, CredError> {
        let mut request = self.client.get(self.url(url));
        request = match auth {
            Auth::None => request,
            Auth::Bearer(token) => request.bearer_auth(token),
            Auth::PrivateToken(token) => request.header("PRIVATE-TOKEN", token),
        };
        self.send(request, self.timeout).await
    }
}

pub(crate) enum Auth<'a> {
    None,
    Bearer(&'a str),
    PrivateToken(&'a str),
}

/// A PKCE pair and a `state` (Fixed constants: S256, 64 characters; 32 bytes base64url).
pub(crate) struct Pkce {
    pub verifier: Zeroizing<String>,
    pub challenge: String,
    pub state: Zeroizing<String>,
}

pub(crate) fn pkce() -> Result<Pkce, CredError> {
    let mut verifier = Zeroizing::new(String::with_capacity(VERIFIER_CHARS));
    let mut byte = [0u8; 1];
    while verifier.len() < VERIFIER_CHARS {
        random_fill(&mut byte)?;
        // Rejection sampling keeps every character equally likely (66 × 3 = 198 < 256).
        let limit = (VERIFIER_ALPHABET.len() * 3) as u8;
        if byte[0] < limit {
            let at = usize::from(byte[0]) % VERIFIER_ALPHABET.len();
            verifier.push(char::from(VERIFIER_ALPHABET[at]));
        }
    }
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut state_bytes = Zeroizing::new([0u8; 32]);
    random_fill(state_bytes.as_mut())?;
    let state = Zeroizing::new(URL_SAFE_NO_PAD.encode(state_bytes.as_ref()));
    Ok(Pkce {
        verifier,
        challenge,
        state,
    })
}

/// The token endpoint's answer, read from the provider's JSON.
pub(crate) struct Tokens {
    pub access_token: Zeroizing<String>,
    pub refresh_token: Option<Zeroizing<String>>,
    pub expires_at: Option<Timestamp>,
    pub scopes: Option<Vec<String>>,
    pub id_token: Option<Zeroizing<String>>,
    pub token_type: String,
    /// Slack: `team.name` and `authed_user.id`.
    pub slack_team: Option<String>,
    pub slack_user: Option<String>,
}

/// How a token-endpoint call ended.
pub(crate) enum Outcome {
    Tokens(Tokens),
    /// The provider answered with an OAuth `error` (or Slack's `ok: false`), or a 4xx without one.
    Refused {
        error: String,
        status: u16,
    },
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn seconds(value: &Value, key: &str) -> Option<i64> {
    match value.get(key) {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

/// `now + seconds`, or None when the provider gave no lifetime.
pub(crate) fn expiry_after(seconds: Option<i64>) -> Option<Timestamp> {
    let seconds = seconds?;
    let at = Timestamp::now().unix_millis() + seconds.max(0).saturating_mul(1000);
    Timestamp::from_unix_millis(at).ok()
}

/// Reads a token-endpoint reply. 5xx is `CRED_PROVIDER_UNREACHABLE`; Slack nests the user token
/// under `authed_user` and separates scopes with commas.
pub(crate) fn read_tokens(reply: Reply) -> Result<Outcome, CredError> {
    if reply.status >= 500 {
        return fail(code::PROVIDER_UNREACHABLE);
    }
    let body = &reply.body;
    let slack_refused = body.get("ok").and_then(Value::as_bool) == Some(false);
    if let Some(error) = text(body, "error") {
        return Ok(Outcome::Refused {
            error,
            status: reply.status,
        });
    }
    if slack_refused || !(200..300).contains(&reply.status) {
        return Ok(Outcome::Refused {
            error: String::new(),
            status: reply.status,
        });
    }
    let user = body
        .get("authed_user")
        .filter(|u| u.get("access_token").is_some());
    let source = user.unwrap_or(body);
    let Some(access_token) = text(source, "access_token") else {
        return Ok(Outcome::Refused {
            error: String::new(),
            status: reply.status,
        });
    };
    let scopes = text(source, "scope").map(|s| {
        s.split([' ', ','])
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    });
    Ok(Outcome::Tokens(Tokens {
        access_token: Zeroizing::new(access_token),
        refresh_token: text(source, "refresh_token").map(Zeroizing::new),
        expires_at: expiry_after(seconds(source, "expires_in")),
        scopes,
        id_token: text(body, "id_token").map(Zeroizing::new),
        token_type: text(source, "token_type").unwrap_or_else(|| "bearer".to_owned()),
        slack_team: body.get("team").and_then(|t| text(t, "name")),
        slack_user: user.and_then(|u| text(u, "id")),
    }))
}

/// C.7: a refused code exchange is `CRED_PROVIDER_REFUSED` (`invalid_client`, `unauthorized_client`, and
/// every other refusal of the grant).
pub(crate) fn exchanged(outcome: Outcome) -> Result<Tokens, CredError> {
    match outcome {
        Outcome::Tokens(tokens) => Ok(tokens),
        Outcome::Refused { .. } => fail(code::PROVIDER_REFUSED),
    }
}

/// E.4: `invalid_grant`, or HTTP 400 or 401, refuses the refresh; any other refusal is treated as the
/// provider being unreachable (retryable, `needs_sign_in` unchanged).
pub(crate) fn refresh_refused(error: &str, status: u16) -> bool {
    error == "invalid_grant"
        || status == 400
        || status == 401
        || (status == 200 && !error.is_empty())
}

/// The claims of an ID token's payload, decoded only to read the label (C.8): trust comes from the
/// TLS token endpoint, so the signature is not checked.
pub(crate) fn id_token_claims(id_token: &str) -> Option<Value> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// C.8: `preferred_username`, else `email`, else `sub`.
pub(crate) fn id_token_label(claims: &Value) -> Option<String> {
    ["preferred_username", "email", "sub"]
        .iter()
        .find_map(|key| text(claims, key).filter(|v| !v.is_empty()))
}

/// C.9: the `name` claim, cut to 254 characters.
pub(crate) fn id_token_name(claims: &Value) -> Option<String> {
    text(claims, "name")
        .filter(|n| !n.is_empty())
        .map(|n| n.chars().take(254).collect())
}

/// The device authorisation endpoint's answer (C.6).
pub(crate) struct DeviceGrant {
    pub device_code: Zeroizing<String>,
    pub user_code: Zeroizing<String>,
    pub verification_uri: String,
    pub expires_at: Timestamp,
    pub interval: Duration,
}

pub(crate) fn read_device_grant(reply: Reply) -> Result<DeviceGrant, CredError> {
    if reply.status >= 500 {
        return fail(code::PROVIDER_UNREACHABLE);
    }
    let body = &reply.body;
    if !(200..300).contains(&reply.status) || body.get("error").is_some() {
        return fail(code::PROVIDER_REFUSED);
    }
    let (Some(device_code), Some(user_code)) = (text(body, "device_code"), text(body, "user_code"))
    else {
        return fail(code::PROVIDER_REFUSED);
    };
    let verification_uri = text(body, "verification_uri_complete")
        .or_else(|| text(body, "verification_uri"))
        // Google names the field `verification_url`.
        .or_else(|| text(body, "verification_url"));
    let Some(verification_uri) = verification_uri else {
        return fail(code::PROVIDER_REFUSED);
    };
    let expires_at = expiry_after(seconds(body, "expires_in"))
        .ok_or_else(|| CredError::of(code::PROVIDER_REFUSED))?;
    let interval = seconds(body, "interval").unwrap_or(5).max(0);
    Ok(DeviceGrant {
        device_code: Zeroizing::new(device_code),
        user_code: Zeroizing::new(user_code),
        verification_uri,
        expires_at,
        interval: Duration::from_secs(interval as u64),
    })
}

/// The device grant type of RFC 8628.
pub(crate) const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
