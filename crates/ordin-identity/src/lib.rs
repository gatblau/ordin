//! Identity and Credentials (IdentityAndCredentials.md): signs the person in to the external systems
//! ordin reaches with the person's own account, keeps every token and password in the platform secret
//! store, refreshes tokens before they expire, hands a usable token only to the module that asked, and
//! owns the secret store every other module uses for named secrets.
//!
//! Two cross-cutting parts land here: the server's secret back end (Configuration.md:
//! [`server_secret_backend`], [`SecretBackendConfig`]) and the sign-in and token rules of
//! AuthenticationAndAuthorisation.md. The crate's two tables are exported as migration data
//! ([`MODULE_NAME`], [`SQLITE_MIGRATIONS`]); the program registers them with the Journal's registry,
//! because this phase-1 crate has no Journal edge (DatabaseMigrations.md rule 2 of *Modules*).
#![forbid(unsafe_code)]

mod accounts;
mod certificate;
mod config;
mod error;
mod loopback;
mod named_secrets;
mod oauth;
mod secret;
mod secret_file;
mod service;
mod sign_in;
mod store;
mod types;

#[cfg(test)]
mod tests;

pub use config::{
    server_secret_backend, BrowserOpener, IdentityConfig, KeySource, Program, SecretBackendConfig,
};
pub use error::{CredError, ALL_CODES};
pub use secret::{SecretBackendKind, SecretName, SecretStore, SecretString};
pub use service::IdentityService;
pub use types::{
    AccessToken, ClientCertificateView, CredentialView, DesktopKeyStore, JiraAccess,
    JiraCredentialMethod, MailPasswordKind, PersonProfile, ProviderConfig, RefreshReport,
    ServerSecrets, SignInChallenge, SignInChallengeId, SignInChanged, SignInMethod, SignInRequest,
    SignInState, JIRA_OAUTH_SCOPES,
};

/// The module name under which the program registers this crate's migrations.
pub const MODULE_NAME: &str = "identity";

/// (version, description, SQL) — registered by the program with the Journal's MigrationRegistry as
/// `ModuleMigrations { module: MODULE_NAME, sqlite: SQLITE_MIGRATIONS, postgres: &[] }`.
pub const SQLITE_MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "create credentials and credential_meta",
        store::MIGRATION_1_SQL,
    ),
    // step 2: amended January 2026, ADR-106 (a widening rebuild, DatabaseMigrations rule 12a).
    (2, "jira credential kind", store::MIGRATION_2_SQL),
];
