//! The caller-authentication seam of the Service layer (C9).
//!
//! `Service` authenticates every call through this trait rather than through the concrete OIDC
//! [`Authenticator`](crate::auth::Authenticator), so an embedding host can supply its own source of
//! [`ScopeContext`] — for instance one that maps a per-call in-process bearer to a context the host
//! built itself — without building the OIDC discovery and JWKS machinery. The OIDC `Authenticator`
//! (feature `http`) is one implementation; the standalone binaries use it.
//!
//! The contract is the one `Authenticator::validate` already honours: an empty bearer is
//! `AuthError::MissingToken`, an unrecognised or invalid bearer is `AuthError::InvalidToken`, and the
//! returned context carries `correlation_id`. Authorisation of the operation class stays in the
//! Service; an implementation never returns `AuthError::InsufficientScope`.

use async_trait::async_trait;

use crate::auth::AuthError;
use crate::types::scope::ScopeContext;

/// Turns a raw bearer value into the authenticated [`ScopeContext`] of the call. The only trusted
/// source of caller identity the Service consults; a request body never supplies scope.
#[async_trait]
pub trait CallerAuthenticator: Send + Sync {
    async fn validate(&self, bearer: &str, correlation_id: &str)
        -> Result<ScopeContext, AuthError>;
}
