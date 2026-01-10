//! The public value types of the sign-in, token, Jira and secret interfaces (Public Interface).

use ordin_types::domain::identity::{Credential, CredentialKind};
use ordin_types::time::Timestamp;
use ordin_types::CredentialId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::CredError;
use crate::secret::SecretString;

/// One pending sign-in. UUIDv7 (chosen here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SignInChallengeId(pub Uuid);

/// The provider values the program builds from the active profile.
#[derive(Debug, Clone)]
pub enum ProviderConfig {
    /// host: "github.com" or a GitHub Enterprise host; client id from IdentityConfig.
    Github {
        host: String,
    },
    /// host: profile `gitlab.host` (default "gitlab.com"); client_id: profile `gitlab.client_id`
    /// (D9, SA-CLOSE-01). Signs in by the device flow only; without a client id the program
    /// offers `save_gitlab_token` instead and never builds this variant.
    Gitlab {
        host: String,
        client_id: String,
    },
    Oidc {
        issuer: String,
        client_id: String,
        scopes: Vec<String>,
    },
    Microsoft {
        tenant_id: String,
        client_id: String,
    },
    /// `client_secret` is the non-confidential secret of a Desktop OAuth client (profile `google.client_secret`).
    Google {
        client_id: String,
        client_secret: Option<String>,
    },
    Slack {
        workspace: String,
        client_id: String,
    },
    Confluence {
        site: String,
        client_id: String,
    },
    /// Jira Cloud through Atlassian OAuth (amended January 2026, ADR-106): `site` is the person's Jira site
    /// as `https://{name}.atlassian.net`; `client_id` is the profile's `confluence.oauth_client_id`.
    Jira {
        site: String,
        client_id: String,
    },
    /// Company mail server single sign-on: tokens from the organisation's OIDC provider (assumed here, SA-CRED-12).
    MailServerSso {
        issuer: String,
        client_id: String,
        scopes: Vec<String>,
        mail_account: String,
    },
}

impl ProviderConfig {
    pub(crate) fn kind(&self) -> CredentialKind {
        match self {
            ProviderConfig::Github { .. } => CredentialKind::Github,
            ProviderConfig::Gitlab { .. } => CredentialKind::Gitlab,
            ProviderConfig::Oidc { .. } => CredentialKind::Oidc,
            ProviderConfig::Microsoft { .. } => CredentialKind::Microsoft,
            ProviderConfig::Google { .. } => CredentialKind::Google,
            ProviderConfig::Slack { .. } => CredentialKind::Slack,
            ProviderConfig::Confluence { .. } => CredentialKind::Confluence,
            ProviderConfig::Jira { .. } => CredentialKind::Jira,
            ProviderConfig::MailServerSso { .. } => CredentialKind::MailServerSingleSignOn,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInMethod {
    Browser,
    DeviceCode,
    /// Personal server only (decision 82): the server builds the authorisation URL and keeps the
    /// PKCE verifier and `state`; the paired desktop opens the URL in the person's browser with its
    /// own loopback listener and forwards the code. Slack only (chosen here).
    RelayedBrowser,
}

impl SignInMethod {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SignInMethod::Browser => "browser",
            SignInMethod::DeviceCode => "device_code",
            SignInMethod::RelayedBrowser => "relayed_browser",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SignInRequest {
    pub provider: ProviderConfig,
    /// Scopes the caller needs (e.g. Graph `Files.ReadWrite.All`); `offline_access` is added for OIDC-style providers.
    pub scopes: Vec<String>,
    /// Desktop: Browser. Server: DeviceCode, or RelayedBrowser for `Slack`. Chosen by the program.
    pub method: SignInMethod,
    /// RelayedBrowser only: the loopback port the paired desktop has bound, one of 47821–47823.
    pub relay_port: Option<u16>,
    /// Words naming who needs it, shown by the caller ("Release planner"); never parsed here.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignInChallenge {
    /// The system browser was opened at the provider's page; wait for `wait_for_sign_in`.
    BrowserOpened {
        challenge_id: SignInChallengeId,
        expires_at: Timestamp,
    },
    /// Show `verification_uri` and `user_code` to the person (chat or screen).
    DeviceCode {
        challenge_id: SignInChallengeId,
        verification_uri: String,
        user_code: String,
        expires_at: Timestamp,
    },
    /// An active credential already covers the requested scopes: nothing is asked.
    AlreadySignedIn { credential: Credential },
    /// Personal server, `RelayedBrowser`: the paired desktop opens `authorisation_url` in the
    /// person's browser, receives the redirect on its own listener at `relay_port`, and calls
    /// `complete_relayed_sign_in` with the code and state. Then `wait_for_sign_in` as usual.
    RelayedBrowser {
        challenge_id: SignInChallengeId,
        authorisation_url: String,
        expires_at: Timestamp,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailPasswordKind {
    MailPassword,
    AppPassword,
}

pub struct AccessToken {
    pub token: SecretString,
    pub expires_at: Option<Timestamp>,
    pub credential_id: CredentialId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInState {
    SignedOut,
    SignedIn,
    NeedsSignIn,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CredentialView {
    pub id: CredentialId,
    pub kind: CredentialKind,
    pub account_label: String,
    pub state: SignInState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RefreshReport {
    pub refreshed: u32,
    pub needs_sign_in: Vec<CredentialId>,
    pub unreachable: Vec<CredentialId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientCertificateView {
    pub name: String,
    /// The leaf certificate's subject common name, ≤ 254 characters.
    pub subject: String,
    pub not_after: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PersonProfile {
    /// The ID token's `name` claim, else the GitHub profile name, else the email's local part.
    pub name: String,
    /// The account label of the active `Oidc`, `Microsoft` or `Google` credential, in that order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
}

// spec: ordin-desktop/components/IdentityAndCredentials#SA-CLOSE-03
#[derive(Debug, Clone, Serialize)]
pub struct SignInChanged {
    pub credential_id: CredentialId,
    pub credential_kind: CredentialKind,
    pub state: SignInState,
}

/// The scopes of an OAuth Jira grant (ADR-106).
pub const JIRA_OAUTH_SCOPES: &[&str] = &[
    "read:jira-work",
    "write:jira-work",
    "read:jira-user",
    "offline_access",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JiraCredentialMethod {
    /// A pasted Atlassian API token, sent with Basic authentication as `email:token`.
    ApiToken { email: String },
    /// Atlassian 3LO through the gateway, scopes `JIRA_OAUTH_SCOPES`.
    Oauth,
}

pub struct JiraAccess {
    /// The person's Jira site host, `{name}.atlassian.net`: the credential's account label.
    pub site: String,
    pub method: JiraCredentialMethod,
    /// The API token, or the OAuth access token refreshed when it expires within 5 minutes.
    pub token: AccessToken,
}

/// The Remote API's TLS private key on the server (secret name `remote-api/tls-key`).
#[async_trait::async_trait]
pub trait ServerSecrets: Send + Sync {
    /// The PEM the setup command stored; `Ok(None)` when absent.
    async fn remote_api_tls_key(&self) -> Result<Option<zeroize::Zeroizing<String>>, CredError>;
    /// Stores (replaces) that key.
    async fn put_remote_api_tls_key(
        &self,
        pem: zeroize::Zeroizing<String>,
    ) -> Result<(), CredError>;
    /// Removes it; succeeds when already absent.
    async fn delete_remote_api_tls_key(&self) -> Result<(), CredError>;
}

/// The desktop's per-server keys (secret names `server-connector/desktop-key/<id>`, passed in
/// whole as `keychain_ref`) and the client certificate the profile names.
#[async_trait::async_trait]
pub trait DesktopKeyStore: Send + Sync {
    async fn put_desktop_key(&self, keychain_ref: &str, key: &[u8; 32]) -> Result<(), CredError>;
    async fn get_desktop_key(
        &self,
        keychain_ref: &str,
    ) -> Result<Option<zeroize::Zeroizing<[u8; 32]>>, CredError>;
    async fn delete_desktop_key(&self, keychain_ref: &str) -> Result<(), CredError>;
    /// As `IdentityService::client_certificate` (step H.5).
    async fn client_certificate(
        &self,
        name: &str,
    ) -> Result<Option<std::sync::Arc<rustls::sign::CertifiedKey>>, CredError>;
}
