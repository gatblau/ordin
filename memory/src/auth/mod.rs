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

mod caller;
#[cfg(feature = "http")]
mod oidc;

pub use caller::CallerAuthenticator;
#[cfg(feature = "http")]
pub use oidc::{AuthConfig, AuthStartupError, Authenticator};

use crate::types::scope::ScopeContext;

// Re-export the read-filter predicate and the scope types this component derives, so callers and
// tests use them through `memory::auth` exactly as the C3 spec's Public Interface declares.
pub use crate::types::scope::{can_read, ScopeRef};

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

/// Pure authorisation check. `Ok(())` if the context permits `op`, else
/// `Err(AuthError::InsufficientScope(op))`. No I/O, no logging side effects. The Service applies it to
/// whatever context its `CallerAuthenticator` returned; `Authenticator::authorise` is the same check.
pub(crate) fn authorise(ctx: &ScopeContext, op: Op) -> Result<(), AuthError> {
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
