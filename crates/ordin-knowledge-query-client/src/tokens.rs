//! The seam over the Identity Service: every token this crate asks a person's sign-in for goes
//! through `TokenSource`, so the unit tests can stand in for Identity (as `ordin-jira` does).

use std::sync::Arc;

use async_trait::async_trait;
use ordin_identity::{AccessToken, CredError, IdentityService, SignInChanged, SignInState};
use ordin_types::domain::identity::CredentialKind;
use ordin_types::CredentialId;
use tokio::sync::broadcast;

/// One active credential, as the linking step (PD-A.2) needs to see it.
#[derive(Debug, Clone)]
pub struct LinkCredential {
    pub id: CredentialId,
    pub kind: CredentialKind,
    /// The provider host (a Confluence site host, a GitLab host, `github.com`).
    pub host: String,
    pub needs_sign_in: bool,
}

/// What this crate needs from the Identity Service (KnowledgeQueryClient.md *Shared Context*,
/// *From Identity and Credentials*, and the ADR-115 additions).
#[async_trait]
pub trait TokenSource: Send + Sync {
    /// The person's company access token for one audience.
    async fn audience_token(
        &self,
        issuer: &str,
        client_id: &str,
        audience: &str,
    ) -> Result<AccessToken, CredError>;
    /// Drops the cached token of that audience.
    async fn invalidate_audience(&self, issuer: &str, client_id: &str, audience: &str);
    /// The active credentials.
    async fn credentials(&self) -> Result<Vec<LinkCredential>, CredError>;
    /// The token of one credential by id.
    async fn token_by_id(&self, id: CredentialId) -> Result<AccessToken, CredError>;
    /// The Confluence token for scope `read:me`, on the linking path only.
    async fn confluence_read_me(&self) -> Result<AccessToken, CredError>;
    /// A Microsoft access token for one scope of another app (ADR-115 `access_token_for_scope`).
    async fn scope_token(
        &self,
        kind: CredentialKind,
        scope: &str,
    ) -> Result<AccessToken, CredError>;
    /// Notified on every change of a credential's sign-in state.
    fn subscribe(&self) -> broadcast::Receiver<SignInChanged>;
}

/// `TokenSource` over the real Identity Service.
pub(crate) struct IdentityTokens(pub Arc<IdentityService>);

#[async_trait]
impl TokenSource for IdentityTokens {
    async fn audience_token(
        &self,
        issuer: &str,
        client_id: &str,
        audience: &str,
    ) -> Result<AccessToken, CredError> {
        self.0
            .access_token_for_audience(issuer, client_id, audience)
            .await
    }

    async fn invalidate_audience(&self, issuer: &str, client_id: &str, audience: &str) {
        self.0
            .invalidate_audience_token(issuer, client_id, audience)
            .await
    }

    async fn credentials(&self) -> Result<Vec<LinkCredential>, CredError> {
        let views = self.0.list().await?;
        Ok(views
            .into_iter()
            .map(|view| LinkCredential {
                id: view.id,
                kind: view.kind,
                host: view.provider_host,
                needs_sign_in: view.state != SignInState::SignedIn,
            })
            .collect())
    }

    async fn token_by_id(&self, id: CredentialId) -> Result<AccessToken, CredError> {
        self.0.access_token_by_id(id).await
    }

    async fn confluence_read_me(&self) -> Result<AccessToken, CredError> {
        self.0
            .access_token_for(CredentialKind::Confluence, &["read:me"])
            .await
    }

    async fn scope_token(
        &self,
        kind: CredentialKind,
        scope: &str,
    ) -> Result<AccessToken, CredError> {
        self.0.access_token_for_scope(kind, scope).await
    }

    fn subscribe(&self) -> broadcast::Receiver<SignInChanged> {
        self.0.subscribe()
    }
}
