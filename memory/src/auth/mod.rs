//! C3 — Auth & Scope: the single security boundary of `memory`.
//!
//! Validates the OIDC bearer JWT presented on a request and derives an authenticated
//! [`ScopeContext`] — the only trusted source of caller identity (tenant, user, teams) and
//! per-operation permission. Identity is read from the token's verified claims; the request body is
//! never trusted (ADR-001, ADR-011). The module owns OIDC discovery, the in-memory JWKS cache and
//! its background + rate-limited on-demand refresh, signature and claim validation, the claims ->
//! `ScopeContext` mapping, the per-operation [`Authenticator::authorise`] check, and the reusable
//! [`can_read`] read-filter predicate (re-exported from the shared types).
//!
//! Security invariants (C3 spec §Security): an alg allowlist (RS256/RS384/RS512/ES256/ES384) blocks
//! the `alg=none` downgrade and HS/RS key-confusion; `iss`/`aud`/`exp`/`nbf` are checked with a 60 s
//! skew leeway; the subject, tenant, and `jti` claims must be present and non-empty. The raw token,
//! its signature, and claim values are never stored or logged — only the `jti` is retained, for the
//! audit trail (SA-AUDIT-01). The warm-cache validation path performs zero network I/O.

mod cache;
mod jwks;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{decode, decode_header, Algorithm, Validation};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::types::scope::{FactAuthority, OpSet, ScopeContext};

// Re-export the read-filter predicate and the scope types this component derives, so callers and
// tests use them through `memory::auth` exactly as the C3 spec's Public Interface declares.
pub use crate::types::scope::{can_read, ScopeRef};

use cache::{CachedKey, JwksCache};

/// Clock-skew leeway applied to `exp`/`nbf` (C3 spec step 5; component-local constant).
const CLOCK_SKEW_LEEWAY_SECS: u64 = 60;

/// Minimum interval between on-demand JWKS refreshes on an unknown `kid` (SA-JWKS-01).
const ONDEMAND_REFRESH_MIN_INTERVAL: Duration = Duration::from_secs(60);

/// Per-operation kinds an authenticated caller may be permitted to perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Read,
    Write,
    Forget,
}

/// Errors at the auth boundary. C8 maps each onto `AppError` + a registry code.
#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    /// No bearer token, or the Authorization header is absent/malformed.
    /// -> AppError::Unauthenticated(Missing) -> 401 AUTH_MISSING_TOKEN
    #[error("missing bearer token")]
    MissingToken,
    /// Token present but failed validation (signature, iss, aud, exp/nbf, unknown kid after refresh,
    /// missing/empty required claim, malformed JWT). The String is an operator-facing reason that
    /// never contains token bytes or claim values.
    /// -> AppError::Unauthenticated(Invalid) -> 401 AUTH_INVALID_TOKEN
    #[error("invalid token: {0}")]
    InvalidToken(String),
    /// Authenticated but lacks the required operation scope.
    /// -> AppError::InsufficientScope -> 403 AUTH_INSUFFICIENT_SCOPE
    #[error("insufficient scope: requires {0:?}")]
    InsufficientScope(Op),
}

/// Errors that can occur only at startup (before serving traffic).
#[derive(thiserror::Error, Debug)]
pub enum AuthStartupError {
    #[error("oidc discovery failed: {0}")]
    Discovery(String),
    #[error("jwks fetch failed: {0}")]
    JwksFetch(String),
    #[error("missing required config: {0}")]
    MissingConfig(String),
}

/// Construction-time configuration, populated from environment variables (§2D).
#[derive(Clone)]
pub struct AuthConfig {
    /// `MEMORY_OIDC_ISSUER` (required).
    pub issuer: String,
    /// `MEMORY_OIDC_AUDIENCE` (required).
    pub audience: String,
    /// `MEMORY_OIDC_SUBJECT_CLAIM`, default "sub".
    pub subject_claim: String,
    /// `MEMORY_OIDC_TEAMS_CLAIM`, default "groups".
    pub teams_claim: String,
    /// `MEMORY_OIDC_TENANT_CLAIM`, default "tenant".
    pub tenant_claim: String,
    /// `MEMORY_OIDC_CLIENT_CLAIM`, default "azp" (the OIDC "authorized party").
    pub client_claim: String,
    /// `MEMORY_OIDC_AUTHORITY_CLAIM`, default "memory_fact_authority".
    pub authority_claim: String,
    /// `MEMORY_JWKS_REFRESH_SECS`, default 3600 s.
    pub jwks_refresh: Duration,
}

impl AuthConfig {
    /// Derive the auth configuration from the frozen application [`crate::config::Config`].
    pub fn from_config(config: &crate::config::Config) -> Self {
        AuthConfig {
            issuer: config.oidc_issuer.clone(),
            audience: config.oidc_audience.clone(),
            subject_claim: config.oidc_subject_claim.clone(),
            teams_claim: config.oidc_teams_claim.clone(),
            tenant_claim: config.oidc_tenant_claim.clone(),
            client_claim: config.oidc_client_claim.clone(),
            authority_claim: config.oidc_authority_claim.clone(),
            jwks_refresh: Duration::from_secs(u64::from(config.jwks_refresh_secs)),
        }
    }
}

/// Shared state behind the [`Authenticator`]; held in an `Arc` so clones are cheap and share one cache.
struct AuthState {
    config: AuthConfig,
    /// Resolved during discovery.
    jwks_uri: String,
    /// Read-mostly; writes only on refresh.
    cache: RwLock<JwksCache>,
    /// On-demand refresh rate limiter (60 s). `None` until the first on-demand attempt.
    last_ondemand_refresh: RwLock<Option<Instant>>,
    /// Discovery/JWKS fetch client (5 s timeout).
    http: reqwest::Client,
}

/// Holds the OIDC config and the in-memory JWKS cache; thread-safe and cheap to clone (Arc inside).
#[derive(Clone)]
pub struct Authenticator {
    state: Arc<AuthState>,
}

impl Authenticator {
    /// Performs OIDC discovery against `<issuer>/.well-known/openid-configuration`, fetches the
    /// initial JWKS, and spawns the background refresh task (interval = `config.jwks_refresh`).
    /// Returns `Err` before any traffic is served if config is incomplete or discovery / the first
    /// JWKS fetch fails, so readiness can fail fast.
    pub async fn new(config: AuthConfig) -> Result<Self, AuthStartupError> {
        // Step 1 — validate required config.
        if config.issuer.trim().is_empty() {
            return Err(AuthStartupError::MissingConfig("MEMORY_OIDC_ISSUER".into()));
        }
        if config.audience.trim().is_empty() {
            return Err(AuthStartupError::MissingConfig(
                "MEMORY_OIDC_AUDIENCE".into(),
            ));
        }

        let http = jwks::build_http_client();

        // Step 2 — OIDC discovery.
        let jwks_uri = jwks::discover_jwks_uri(&http, &config.issuer)
            .await
            .map_err(|reason| {
                tracing::error!(
                    target: "memory::auth",
                    event = "oidc_discovery_failed",
                    issuer = %config.issuer,
                    correlation_id = "startup",
                    "oidc discovery failed"
                );
                AuthStartupError::Discovery(reason)
            })?;

        // Step 3 — initial JWKS fetch.
        let keys = jwks::fetch_jwks(&http, &jwks_uri).await.map_err(|reason| {
            tracing::error!(
                target: "memory::auth",
                event = "jwks_fetch_failed",
                attempt_reason = %reason,
                "initial jwks fetch failed"
            );
            AuthStartupError::JwksFetch(reason)
        })?;
        let key_count = keys.len();

        let state = Arc::new(AuthState {
            config: config.clone(),
            jwks_uri,
            cache: RwLock::new(JwksCache::new(keys)),
            last_ondemand_refresh: RwLock::new(None),
            http,
        });

        tracing::info!(
            target: "memory::auth",
            event = "jwks_refreshed",
            key_count,
            "initial jwks loaded"
        );

        // Step 5 — report the authority claim this build reads. This is the start-up half of making
        // an absent control visible: a deployment whose IdP mints a differently-named claim finds out
        // at boot, rather than by measuring a 202 it expected to be a 403. The run-time half is C9's
        // `memory_write_authority_absent_total`.
        // spec: phase-1-analysis#SA-AUTHZ-02
        tracing::info!(
            target: "memory::auth",
            event = "fact_authority_claim_configured",
            claim = %state.config.authority_claim,
            correlation_id = "startup",
            "fact-class write authority reads this claim; a token without it may assert every class"
        );

        // Step 4 — spawn the background refresh task.
        let bg_state = Arc::clone(&state);
        tokio::spawn(async move {
            background_refresh_loop(bg_state).await;
        });

        Ok(Authenticator { state })
    }

    /// Validates a raw bearer token string (the value after "Bearer ") and returns the derived
    /// [`ScopeContext`]. `correlation_id` is the per-request id minted by C8, threaded into the
    /// context for response/audit correlation. Performs no network I/O on the warm-cache path; on an
    /// unknown `kid` it may trigger one rate-limited on-demand JWKS refresh before failing.
    pub async fn validate(
        &self,
        bearer_token: &str,
        correlation_id: &str,
    ) -> Result<ScopeContext, AuthError> {
        // Step 1 — reject an empty token.
        if bearer_token.trim().is_empty() {
            return Err(AuthError::MissingToken);
        }

        // Step 2 — decode the header (no signature) for `alg` and `kid`; enforce the alg allowlist.
        let header = decode_header(bearer_token)
            .map_err(|_| AuthError::InvalidToken("malformed jwt header".into()))?;
        let validation_alg = map_alg_allowlist(header.alg)
            .ok_or_else(|| AuthError::InvalidToken("unsupported alg".into()))?;
        let kid = header
            .kid
            .ok_or_else(|| AuthError::InvalidToken("missing kid".into()))?;

        // Steps 3 & 4 — resolve the key for `kid`, with at most one rate-limited on-demand refresh.
        let key = self.resolve_key(&kid, correlation_id).await?;

        // Step 5 — verify signature + iss/aud/exp/nbf against the matched key.
        // The header's declared alg is honoured for verification, but it is constrained to the same
        // family as the cached key's alg (the allowlist already rejected `none`/HS*); a mismatch
        // between the token alg and the key alg fails verification.
        let claims = verify_claims(
            bearer_token,
            key,
            validation_alg,
            &self.state.config.issuer,
            &self.state.config.audience,
        )?;

        // Steps 6-8 — map claims into a ScopeContext.
        let ctx = map_claims(&claims, &self.state.config, correlation_id)?;

        tracing::debug!(
            target: "memory::auth",
            event = "token_validated",
            tenant = %ctx.tenant,
            user = %ctx.user,
            jti = %ctx.token_jti,
            client = ctx.client.as_deref().unwrap_or("-"),
            correlation_id = %correlation_id,
            "token validated"
        );

        Ok(ctx)
    }

    /// Pure authorisation check. `Ok(())` if the context permits `op`, else
    /// `Err(AuthError::InsufficientScope(op))`. No I/O, no logging side effects.
    pub fn authorise(ctx: &ScopeContext, op: Op) -> Result<(), AuthError> {
        let permitted = match op {
            Op::Read => ctx.allowed_ops.read,
            Op::Write => ctx.allowed_ops.write,
            Op::Forget => ctx.allowed_ops.forget,
        };
        if permitted {
            Ok(())
        } else {
            Err(AuthError::InsufficientScope(op))
        }
    }

    /// Current cached key count, for the `auth_jwks_keys` gauge and tests that assert a warm cache.
    pub async fn cached_key_count(&self) -> usize {
        self.state.cache.read().await.len()
    }

    /// Resolve the verification key for `kid`, triggering at most one rate-limited on-demand refresh
    /// when the cache misses (SA-JWKS-01). Returns the matched key cloned out of the cache so the
    /// read lock is released before the CPU-bound verification.
    async fn resolve_key(&self, kid: &str, correlation_id: &str) -> Result<KeyMaterial, AuthError> {
        if let Some(km) = self.lookup_key(kid).await {
            return Ok(km);
        }

        // Cache miss — attempt one on-demand refresh if the 60 s rate limit allows it.
        let rate_limited = !self.try_ondemand_refresh().await;
        tracing::warn!(
            target: "memory::auth",
            event = "jwks_ondemand_refresh",
            rate_limited,
            correlation_id = %correlation_id,
            "on-demand jwks refresh for unknown kid"
        );
        if rate_limited {
            return Err(AuthError::InvalidToken("unknown signing key".into()));
        }
        self.lookup_key(kid)
            .await
            .ok_or_else(|| AuthError::InvalidToken("unknown signing key".into()))
    }

    /// Clone the key material for `kid` out of the cache, if present.
    async fn lookup_key(&self, kid: &str) -> Option<KeyMaterial> {
        let cache = self.state.cache.read().await;
        cache.get(kid).map(KeyMaterial::from)
    }

    /// Attempt one on-demand JWKS refresh, honouring the 60 s rate limit. Returns `true` if a refresh
    /// was performed (regardless of whether it changed the key set), `false` if the rate limit blocked
    /// it. A failed fetch leaves the previous key set intact.
    async fn try_ondemand_refresh(&self) -> bool {
        {
            let last = self.state.last_ondemand_refresh.read().await;
            if let Some(at) = *last {
                if at.elapsed() < ONDEMAND_REFRESH_MIN_INTERVAL {
                    return false;
                }
            }
        }
        // Stamp the attempt time before fetching so a concurrent caller is rate-limited.
        {
            let mut last = self.state.last_ondemand_refresh.write().await;
            *last = Some(Instant::now());
        }
        match jwks::fetch_jwks(&self.state.http, &self.state.jwks_uri).await {
            Ok(keys) => {
                let key_count = keys.len();
                let mut cache = self.state.cache.write().await;
                *cache = JwksCache::new(keys);
                tracing::info!(
                    target: "memory::auth",
                    event = "jwks_refreshed",
                    key_count,
                    kind = "ondemand",
                    "on-demand jwks refresh succeeded"
                );
            }
            Err(reason) => {
                tracing::warn!(
                    target: "memory::auth",
                    event = "jwks_refresh_failed",
                    attempt_reason = %reason,
                    kind = "ondemand",
                    "on-demand jwks refresh failed; keeping previous keys"
                );
            }
        }
        true
    }
}

/// Verification key material cloned out of the cache so the lock is not held during decoding.
struct KeyMaterial {
    decoding_key: jsonwebtoken::DecodingKey,
    alg: Algorithm,
}

impl From<&CachedKey> for KeyMaterial {
    fn from(k: &CachedKey) -> Self {
        KeyMaterial {
            decoding_key: k.decoding_key.clone(),
            alg: k.alg,
        }
    }
}

/// The decoded JWT claims this component reads. Claim *names* are configurable, so the typed claims
/// captured here are only the always-named-by-spec ones (`exp`/`nbf` are validated by the library);
/// the configurable claims are read from the untyped `Value` map.
#[derive(serde::Deserialize)]
struct RawClaims {
    /// Whole claim set, used to read the configurable subject/tenant/teams/scope/jti claims.
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

/// Map a JWS [`Algorithm`] onto the C3 allowlist, returning `Some(alg)` for the permitted asymmetric
/// algorithms and `None` for everything else — `alg=none` (absent from the enum), and every HS*
/// (symmetric) algorithm, which would enable RS/HS key-confusion. This is the single source of truth
/// for the allowlist, used both at JWK parse time and at token-header check time.
pub(crate) fn map_alg_allowlist(alg: Algorithm) -> Option<Algorithm> {
    match alg {
        Algorithm::RS256
        | Algorithm::RS384
        | Algorithm::RS512
        | Algorithm::ES256
        | Algorithm::ES384 => Some(alg),
        _ => None,
    }
}

/// Verify the token's signature and registered claims (`iss`/`aud`/`exp`/`nbf`) against the matched
/// key, with a 60 s clock-skew leeway. Returns the decoded claim set on success. Every failure maps
/// to `AuthError::InvalidToken` with a reason that names the failed check and contains no token bytes.
fn verify_claims(
    token: &str,
    key: KeyMaterial,
    declared_alg: Algorithm,
    issuer: &str,
    audience: &str,
) -> Result<HashMap<String, Value>, AuthError> {
    // The token's declared alg must match the key's alg (both already inside the allowlist); validate
    // against the key's own algorithm to prevent accepting a token signed with a different family.
    if declared_alg != key.alg {
        return Err(AuthError::InvalidToken("signature".into()));
    }
    let mut validation = Validation::new(key.alg);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.leeway = CLOCK_SKEW_LEEWAY_SECS;
    validation.validate_exp = true;
    validation.validate_nbf = true;

    match decode::<RawClaims>(token, &key.decoding_key, &validation) {
        Ok(data) => Ok(data.claims.extra),
        Err(e) => Err(AuthError::InvalidToken(classify_jwt_error(&e))),
    }
}

/// Reduce a `jsonwebtoken` error to a fixed operator-facing reason naming the failed check; never
/// includes token bytes or claim values.
fn classify_jwt_error(e: &jsonwebtoken::errors::Error) -> String {
    use jsonwebtoken::errors::ErrorKind;
    match e.kind() {
        ErrorKind::InvalidSignature => "signature",
        ErrorKind::InvalidIssuer => "issuer mismatch",
        ErrorKind::InvalidAudience => "audience mismatch",
        ErrorKind::ExpiredSignature => "expired",
        ErrorKind::ImmatureSignature => "not yet valid",
        ErrorKind::InvalidAlgorithm => "unsupported alg",
        _ => "signature",
    }
    .to_string()
}

/// Map a verified claim set into a [`ScopeContext`] (C3 spec steps 6-8). Reads the configurable
/// subject/tenant/teams/scope/jti claims; a missing or empty required claim is an invalid token.
fn map_claims(
    claims: &HashMap<String, Value>,
    config: &AuthConfig,
    correlation_id: &str,
) -> Result<ScopeContext, AuthError> {
    // Step 6 — subject -> user.
    let user = claim_str(claims, &config.subject_claim)
        .ok_or_else(|| AuthError::InvalidToken("missing subject claim".into()))?;
    // Step 6 — tenant.
    let tenant = claim_str(claims, &config.tenant_claim)
        .ok_or_else(|| AuthError::InvalidToken("missing tenant claim".into()))?;
    // Step 6 — teams (array of strings, or a single string coerced to a one-element vector; a missing
    // claim yields an empty vector).
    let teams = claim_string_list(claims, &config.teams_claim);
    // Step 6 — jti (required for the audit trail).
    let token_jti =
        claim_str(claims, "jti").ok_or_else(|| AuthError::InvalidToken("missing jti".into()))?;

    // Step 7 — OAuth scope claim (space-delimited per RFC 6749) -> OpSet.
    let allowed_ops = parse_scope(claims.get("scope").and_then(Value::as_str).unwrap_or(""));

    // Step 8 — calling client identity. An absent claim is None and is never defaulted to the
    // subject: the point of carrying it is to tell two services acting for the same user apart, and
    // substituting the subject would erase exactly that. Present-but-malformed is a refusal.
    // spec: components/auth-scope#SA-AUTHZ-08
    let client = match claims.get(&config.client_claim) {
        None | Some(Value::Null) => None,
        Some(Value::String(v)) if !v.is_empty() => Some(v.clone()),
        Some(_) => return Err(AuthError::InvalidToken("malformed client claim".into())),
    };

    // Step 9 — fact-class write authority, read strictly.
    let fact_authority = parse_fact_authority(claims, &config.authority_claim)?;

    // Step 10 — construct the context. The raw token is never stored.
    Ok(ScopeContext {
        tenant,
        teams,
        user,
        token_jti,
        allowed_ops,
        client,
        fact_authority,
        correlation_id: correlation_id.to_string(),
    })
}

/// Parse the fact-class write-authority claim **strictly**.
///
/// This is the deliberate opposite of [`parse_scope`], which ignores tokens it does not recognise.
/// Forward compatibility is right for a *capability* — an old build that skips an unknown scope
/// simply grants less — and wrong for a *restriction*, where skipping an unknown qualifier leaves
/// the build granting **more** than the token's issuer intended. So every departure from the
/// expected shape is a refusal, including an unknown key inside an otherwise valid entry.
///
/// Three outcomes, and the middle one is not the same as the first:
/// - claim absent            -> `None`          (may assert every class)
/// - claim present as `[]`   -> `Some(vec![])`  (may assert no class)
/// - claim present with rows -> `Some(entries)`
///
/// The reason string is operator-facing and reaches the log and the typed error only; the HTTP body
/// stays the generic `AUTH_INVALID_TOKEN` text, so a caller never learns which claim shape this
/// build expects. Claim *contents* are never reproduced in the reason — a class vocabulary belongs
/// to the caller.
// spec: components/auth-scope#SA-AUTHZ-03
fn parse_fact_authority(
    claims: &HashMap<String, Value>,
    claim_name: &str,
) -> Result<Option<Vec<FactAuthority>>, AuthError> {
    const BAD: &str = "malformed memory_fact_authority claim";

    let raw = match claims.get(claim_name) {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => v,
    };
    let Some(items) = raw.as_array() else {
        return Err(AuthError::InvalidToken(format!("{BAD}: expected array")));
    };

    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let Some(obj) = item.as_object() else {
            return Err(AuthError::InvalidToken(format!(
                "{BAD}: element is not an object"
            )));
        };
        // An unrecognised key is refused rather than ignored: a future narrowing qualifier that an
        // older build skipped would widen that build's authority silently.
        if let Some(k) = obj.keys().find(|k| k.as_str() != "class" && k.as_str() != "source") {
            return Err(AuthError::InvalidToken(format!("{BAD}: unknown key {k}")));
        }
        let class = match obj.get("class").and_then(Value::as_str) {
            Some(c) if !c.is_empty() => c.to_string(),
            _ => {
                return Err(AuthError::InvalidToken(format!(
                    "{BAD}: missing or empty class"
                )))
            }
        };
        let source = match obj.get("source") {
            None | Some(Value::Null) => None,
            Some(Value::String(v)) if !v.is_empty() => Some(v.clone()),
            Some(_) => return Err(AuthError::InvalidToken(format!("{BAD}: invalid source"))),
        };
        entries.push(FactAuthority { class, source });
    }
    Ok(Some(entries))
}

/// Read a claim as a non-empty string, returning `None` for absent, non-string, or empty values.
fn claim_str(claims: &HashMap<String, Value>, name: &str) -> Option<String> {
    claims
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// Read a claim as a list of strings: a JSON array of strings, or a single string coerced to a
/// one-element vector. A missing, null, or otherwise-typed claim yields an empty vector.
fn claim_string_list(claims: &HashMap<String, Value>, name: &str) -> Vec<String> {
    match claims.get(name) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Parse the space-delimited OAuth `scope` claim into an [`OpSet`]. Unknown scope tokens are ignored
/// (forward compatibility); an absent scope grants nothing.
fn parse_scope(scope: &str) -> OpSet {
    let mut ops = OpSet {
        read: false,
        write: false,
        forget: false,
    };
    for token in scope.split_whitespace() {
        match token {
            "memory.read" => ops.read = true,
            "memory.write" => ops.write = true,
            "memory.forget" => ops.forget = true,
            _ => {}
        }
    }
    ops
}

/// Background refresh loop: on each `config.jwks_refresh` tick, re-fetch the JWKS; on success replace
/// the cached key set atomically; on failure keep the previous set and retry on the next tick.
async fn background_refresh_loop(state: Arc<AuthState>) {
    let mut ticker = tokio::time::interval(state.config.jwks_refresh);
    // The first tick fires immediately; skip it since `new` already loaded the initial set.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        match jwks::fetch_jwks(&state.http, &state.jwks_uri).await {
            Ok(keys) => {
                let key_count = keys.len();
                let mut cache = state.cache.write().await;
                *cache = JwksCache::new(keys);
                tracing::info!(
                    target: "memory::auth",
                    event = "jwks_refreshed",
                    key_count,
                    kind = "background",
                    "background jwks refresh succeeded"
                );
            }
            Err(reason) => {
                tracing::warn!(
                    target: "memory::auth",
                    event = "jwks_refresh_failed",
                    attempt_reason = %reason,
                    kind = "background",
                    "background jwks refresh failed; keeping previous keys"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_ops(read: bool, write: bool, forget: bool) -> ScopeContext {
        ScopeContext {
            tenant: "acme".into(),
            teams: vec!["platform".into()],
            user: "user-42".into(),
            token_jti: "jti-1".into(),
            allowed_ops: OpSet {
                read,
                write,
                forget,
            },
            client: None,
            fact_authority: None,
            correlation_id: "c1".into(),
        }
    }

    #[test]
    fn alg_allowlist_admits_asymmetric_rejects_symmetric() {
        for alg in [
            Algorithm::RS256,
            Algorithm::RS384,
            Algorithm::RS512,
            Algorithm::ES256,
            Algorithm::ES384,
        ] {
            assert_eq!(map_alg_allowlist(alg), Some(alg), "{alg:?} must be allowed");
        }
        for alg in [Algorithm::HS256, Algorithm::HS384, Algorithm::HS512] {
            assert_eq!(map_alg_allowlist(alg), None, "{alg:?} must be rejected");
        }
    }

    #[test]
    fn scope_claim_maps_to_opset() {
        let ops = parse_scope("memory.read memory.write");
        assert!(ops.read && ops.write && !ops.forget);

        let all = parse_scope("memory.read memory.write memory.forget");
        assert!(all.read && all.write && all.forget);

        let none = parse_scope("");
        assert!(!none.read && !none.write && !none.forget);

        // Unknown tokens are ignored (forward compatibility).
        let mixed = parse_scope("memory.read offline_access openid");
        assert!(mixed.read && !mixed.write && !mixed.forget);
    }

    #[test]
    fn authorise_grants_present_op_denies_absent_op() {
        let ctx = ctx_with_ops(true, true, false);
        assert!(Authenticator::authorise(&ctx, Op::Read).is_ok());
        assert!(Authenticator::authorise(&ctx, Op::Write).is_ok());
        match Authenticator::authorise(&ctx, Op::Forget) {
            Err(AuthError::InsufficientScope(Op::Forget)) => {}
            other => panic!("expected InsufficientScope(Forget), got {other:?}"),
        }
    }

    // --- Strict fact-class authority parsing (SA-AUTHZ-03) and client identity (SA-AUTHZ-08) ---

    fn authority_claims(v: Value) -> HashMap<String, Value> {
        let mut c = HashMap::new();
        c.insert("memory_fact_authority".to_string(), v);
        c
    }

    #[test]
    fn authority_claim_absent_is_none_not_empty() {
        // Absent means "every class". It must not collapse into `Some(vec![])`, which means the
        // opposite — no class at all.
        let got = parse_fact_authority(&HashMap::new(), "memory_fact_authority").unwrap();
        assert!(got.is_none());
        let got = parse_fact_authority(&authority_claims(Value::Null), "memory_fact_authority")
            .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn authority_claim_empty_array_is_authority_over_nothing() {
        let got = parse_fact_authority(&authority_claims(Value::Array(vec![])), "memory_fact_authority")
            .unwrap();
        assert_eq!(got, Some(vec![]));
    }

    #[test]
    fn authority_claim_parses_class_and_optional_source() {
        let raw = serde_json::json!([
            {"class": "medication-authoritative", "source": "nhs-uk-plugin"},
            {"class": "adherence-report"}
        ]);
        let got = parse_fact_authority(&authority_claims(raw), "memory_fact_authority")
            .unwrap()
            .expect("claim present");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].class, "medication-authoritative");
        assert_eq!(got[0].source.as_deref(), Some("nhs-uk-plugin"));
        assert_eq!(got[1].class, "adherence-report");
        assert_eq!(got[1].source, None);
    }

    #[test]
    fn malformed_authority_claim_is_refused_not_ignored() {
        // This is the whole point of the strict reading: every one of these would, under
        // `parse_scope`'s forward-compatible treatment, leave the build granting MORE authority
        // than the token's issuer wrote down.
        let bad = [
            serde_json::json!("medication-authoritative"), // a string, not an array
            serde_json::json!({"class": "a"}),             // an object, not an array
            serde_json::json!(7),                          // a number
            serde_json::json!(true),                       // a boolean
            serde_json::json!(["a"]),                      // element is not an object
            serde_json::json!([{}]),                       // element missing `class`
            serde_json::json!([{"class": ""}]),            // empty class
            serde_json::json!([{"class": 1}]),             // class is not a string
            serde_json::json!([{"class": "a", "source": ""}]), // empty source
            serde_json::json!([{"class": "a", "source": 2}]),  // source is not a string
            serde_json::json!([{"class": "a", "tenant_scope": "t-1"}]), // unknown key
        ];
        for raw in bad {
            let err = parse_fact_authority(&authority_claims(raw.clone()), "memory_fact_authority")
                .expect_err(&format!("must refuse {raw}"));
            let AuthError::InvalidToken(reason) = err else {
                panic!("must be InvalidToken for {raw}");
            };
            // The operator-facing reason names the claim so a misconfigured issuer is diagnosable;
            // the HTTP body stays the generic AUTH_INVALID_TOKEN text, checked separately.
            assert!(
                reason.contains("memory_fact_authority"),
                "reason must name the claim, got {reason:?}"
            );
        }
    }

    #[test]
    fn authority_claim_reason_never_reproduces_a_class_vocabulary() {
        // A class vocabulary belongs to the caller; the refusal reason describes the shape
        // violation, not the value that violated it.
        let raw = serde_json::json!([{"class": "medication-authoritative", "secret_qualifier": "x"}]);
        let err = parse_fact_authority(&authority_claims(raw), "memory_fact_authority")
            .expect_err("unknown key is refused");
        let AuthError::InvalidToken(reason) = err else {
            panic!("InvalidToken");
        };
        assert!(!reason.contains("medication-authoritative"), "got {reason:?}");
    }

    #[test]
    fn client_claim_absent_is_none_and_is_never_the_subject() {
        let mut claims = HashMap::new();
        claims.insert("sub".into(), Value::String("user-42".into()));
        claims.insert("tenant".into(), Value::String("acme".into()));
        claims.insert("jti".into(), Value::String("jti-9".into()));
        let ctx = map_claims(&claims, &test_config(), "corr-1").unwrap();
        assert_eq!(ctx.client, None, "absent azp is None");
        assert_ne!(ctx.client.as_deref(), Some(ctx.user.as_str()));
        assert_eq!(ctx.fact_authority, None, "absent claim is every class");
    }

    #[test]
    fn client_claim_is_read_and_malformed_is_refused() {
        let mut claims = HashMap::new();
        claims.insert("sub".into(), Value::String("user-42".into()));
        claims.insert("tenant".into(), Value::String("acme".into()));
        claims.insert("jti".into(), Value::String("jti-9".into()));
        claims.insert("azp".into(), Value::String("health-integration".into()));
        let ctx = map_claims(&claims, &test_config(), "corr-1").unwrap();
        assert_eq!(ctx.client.as_deref(), Some("health-integration"));

        // `ScopeContext` deliberately carries no `Debug` (it holds caller identity, and a derived
        // Debug is one careless `{:?}` away from a log line), so match on the result rather than
        // reaching for `expect_err`.
        claims.insert("azp".into(), Value::Number(7.into()));
        assert!(matches!(
            map_claims(&claims, &test_config(), "corr-1"),
            Err(AuthError::InvalidToken(_))
        ));
    }

    fn test_config() -> AuthConfig {
        AuthConfig {
            issuer: "https://issuer.test".into(),
            audience: "memory".into(),
            subject_claim: "sub".into(),
            teams_claim: "groups".into(),
            tenant_claim: "tenant".into(),
            client_claim: "azp".into(),
            authority_claim: "memory_fact_authority".into(),
            jwks_refresh: Duration::from_secs(3600),
        }
    }

    #[test]
    fn map_claims_reads_subject_tenant_teams_and_jti() {
        let mut claims = HashMap::new();
        claims.insert("sub".into(), Value::String("user-42".into()));
        claims.insert("tenant".into(), Value::String("acme".into()));
        claims.insert(
            "groups".into(),
            Value::Array(vec![
                Value::String("platform".into()),
                Value::String("sre".into()),
            ]),
        );
        claims.insert("jti".into(), Value::String("jti-9".into()));
        claims.insert(
            "scope".into(),
            Value::String("memory.read memory.write".into()),
        );

        let config = AuthConfig {
            issuer: "iss".into(),
            audience: "aud".into(),
            subject_claim: "sub".into(),
            teams_claim: "groups".into(),
            tenant_claim: "tenant".into(),
            client_claim: "azp".into(),
            authority_claim: "memory_fact_authority".into(),
            jwks_refresh: Duration::from_secs(3600),
        };
        let ctx = map_claims(&claims, &config, "c-7").expect("maps cleanly");
        assert_eq!(ctx.user, "user-42");
        assert_eq!(ctx.tenant, "acme");
        assert_eq!(ctx.teams, vec!["platform".to_string(), "sre".to_string()]);
        assert_eq!(ctx.token_jti, "jti-9");
        assert!(ctx.allowed_ops.read && ctx.allowed_ops.write && !ctx.allowed_ops.forget);
        assert_eq!(ctx.correlation_id, "c-7");
    }

    #[test]
    fn map_claims_rejects_missing_required_claims() {
        let config = AuthConfig {
            issuer: "iss".into(),
            audience: "aud".into(),
            subject_claim: "sub".into(),
            teams_claim: "groups".into(),
            tenant_claim: "tenant".into(),
            client_claim: "azp".into(),
            authority_claim: "memory_fact_authority".into(),
            jwks_refresh: Duration::from_secs(3600),
        };

        // Missing subject.
        let mut claims = HashMap::new();
        claims.insert("tenant".into(), Value::String("acme".into()));
        claims.insert("jti".into(), Value::String("j".into()));
        assert!(matches!(
            map_claims(&claims, &config, "c"),
            Err(AuthError::InvalidToken(ref r)) if r.contains("subject")
        ));

        // Missing tenant.
        let mut claims = HashMap::new();
        claims.insert("sub".into(), Value::String("u".into()));
        claims.insert("jti".into(), Value::String("j".into()));
        assert!(matches!(
            map_claims(&claims, &config, "c"),
            Err(AuthError::InvalidToken(ref r)) if r.contains("tenant")
        ));

        // Missing jti.
        let mut claims = HashMap::new();
        claims.insert("sub".into(), Value::String("u".into()));
        claims.insert("tenant".into(), Value::String("acme".into()));
        assert!(matches!(
            map_claims(&claims, &config, "c"),
            Err(AuthError::InvalidToken(ref r)) if r.contains("jti")
        ));
    }

    #[test]
    fn teams_claim_accepts_single_string_and_empty_default() {
        let config = AuthConfig {
            issuer: "iss".into(),
            audience: "aud".into(),
            subject_claim: "sub".into(),
            teams_claim: "groups".into(),
            tenant_claim: "tenant".into(),
            client_claim: "azp".into(),
            authority_claim: "memory_fact_authority".into(),
            jwks_refresh: Duration::from_secs(3600),
        };

        let mut claims = HashMap::new();
        claims.insert("sub".into(), Value::String("u".into()));
        claims.insert("tenant".into(), Value::String("acme".into()));
        claims.insert("jti".into(), Value::String("j".into()));
        claims.insert("groups".into(), Value::String("solo".into()));
        let ctx = map_claims(&claims, &config, "c").unwrap();
        assert_eq!(ctx.teams, vec!["solo".to_string()]);

        // Missing teams claim -> empty vec.
        claims.remove("groups");
        let ctx = map_claims(&claims, &config, "c").unwrap();
        assert!(ctx.teams.is_empty());
    }

    #[tokio::test]
    async fn new_rejects_empty_issuer() {
        let config = AuthConfig {
            issuer: "".into(),
            audience: "aud".into(),
            subject_claim: "sub".into(),
            teams_claim: "groups".into(),
            tenant_claim: "tenant".into(),
            client_claim: "azp".into(),
            authority_claim: "memory_fact_authority".into(),
            jwks_refresh: Duration::from_secs(3600),
        };
        match Authenticator::new(config).await {
            Err(AuthStartupError::MissingConfig(v)) => assert_eq!(v, "MEMORY_OIDC_ISSUER"),
            Err(other) => panic!("expected MissingConfig, got {other:?}"),
            Ok(_) => panic!("expected MissingConfig, got Ok"),
        }
    }
}
