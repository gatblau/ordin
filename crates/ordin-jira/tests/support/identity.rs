//! A real Identity Service over a real SQLite file and an encrypted secret file, for the
//! integration tests. The operating system's keychain is never touched.

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ordin_identity::{
    IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig, SecretString,
    SQLITE_MIGRATIONS,
};
use sqlx::sqlite::SqliteConnectOptions;

use super::fixtures::{ACCOUNT_PRIYA, EMAIL, SITE, TOKEN};
use super::TempDir;

/// A desktop Identity Service with no credential.
pub async fn identity(dir: &TempDir) -> Arc<IdentityService> {
    let key_file = dir.path().join("secrets.key");
    std::fs::write(&key_file, STANDARD.encode([7u8; 32])).expect("key file");
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("identity.db"))
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.expect("pool");
    for (_, _, sql) in SQLITE_MIGRATIONS {
        sqlx::raw_sql(*sql)
            .execute(&pool)
            .await
            .expect("identity migration");
    }
    let config = IdentityConfig {
        program: Program::Desktop,
        data_root: dir.path().to_path_buf(),
        keychain_service: "io.gatblau.ordin.jira-test".to_owned(),
        secret_backend: SecretBackendConfig::EncryptedFile {
            key_source: KeySource::KeyFile(key_file),
        },
        github_app_client_id: None,
        microsoft_client_id: None,
        http: reqwest::Client::new(),
        browser: None,
    };
    Arc::new(
        IdentityService::new(config, Some(pool))
            .await
            .expect("the identity service starts"),
    )
}

/// The same, holding the person's Jira API token for `acme.atlassian.net`.
pub async fn identity_with_token(dir: &TempDir) -> Arc<IdentityService> {
    let service = identity(dir).await;
    service
        .save_jira_token(
            SITE,
            EMAIL,
            SecretString::new(TOKEN.to_owned()),
            ACCOUNT_PRIYA,
            "Priya Shah",
        )
        .await
        .expect("the token is kept");
    service
}
