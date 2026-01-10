//! X1: one test per row of the Error Table, provoking the condition and asserting the code, the
//! retryability and the repair action of the *Response Body* column.

use std::sync::atomic::Ordering;

use ordin_types::domain::identity::CredentialKind;
use ordin_types::{CodedError as _, CredentialId, RepairAction};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::harness::{
    code_of, in_minutes, key_file, migrated_pool, temp_dir, Harness, Seed, Setup, Store, PORTS,
};
use crate::config::{IdentityConfig, KeySource, Program, SecretBackendConfig};
use crate::error::CredError;
use crate::secret::{SecretName, SecretString};
use crate::service::IdentityService;
use crate::types::{
    MailPasswordKind, ProviderConfig, SignInChallenge, SignInMethod, SignInRequest,
};

fn assert_error<T>(
    result: Result<T, CredError>,
    code: &str,
    retryable: bool,
    repair: Option<RepairAction>,
) {
    let Err(error) = result else {
        panic!("expected {code}, got Ok");
    };
    assert_eq!(error.code(), code);
    assert_eq!(error.retryable(), retryable, "{code} retryable");
    assert_eq!(error.repair(), repair, "{code} repair");
}

fn config(program: Program, backend: SecretBackendConfig, dir: &std::path::Path) -> IdentityConfig {
    IdentityConfig {
        program,
        data_root: dir.to_path_buf(),
        keychain_service: "io.gatblau.ordin.test".to_owned(),
        secret_backend: backend,
        github_app_client_id: None,
        http: reqwest::Client::new(),
        browser: None,
    }
}

fn request(provider: ProviderConfig, method: SignInMethod) -> SignInRequest {
    SignInRequest {
        provider,
        scopes: Vec::new(),
        method,
        relay_port: None,
        reason: None,
    }
}

fn microsoft() -> ProviderConfig {
    ProviderConfig::Microsoft {
        tenant_id: "4a1c-tenant".to_owned(),
        client_id: "9f0e-app".to_owned(),
    }
}

fn slack() -> ProviderConfig {
    ProviderConfig::Slack {
        workspace: "acme".to_owned(),
        client_id: "1234567890.9876543210".to_owned(),
    }
}

async fn github_device_mock(h: &Harness, expires_in: i64) {
    Mock::given(method("POST"))
        .and(path("/github.com/login/device/code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "dc-1", "user_code": "WDJB-MJHT",
            "verification_uri": "https://github.com/login/device",
            "expires_in": expires_in, "interval": 1
        })))
        .mount(&h.mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/github.com/login/oauth/access_token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"error": "authorization_pending"})),
        )
        .mount(&h.mock)
        .await;
}

// Row: Config: back end and program disagree, pool missing.
#[tokio::test]
async fn config_invalid() {
    let dir = temp_dir();
    let pool = migrated_pool(&dir).await;
    let keychain_on_server = config(
        Program::PersonalServer,
        SecretBackendConfig::OsKeychain,
        &dir,
    );
    let result = IdentityService::new(keychain_on_server, Some(pool)).await;
    assert_error(result.map(|_| ()), "CRED_CONFIG_INVALID", false, None);
    let no_pool = config(Program::Desktop, SecretBackendConfig::OsKeychain, &dir);
    let result = IdentityService::new(no_pool, None).await;
    assert_error(result.map(|_| ()), "CRED_CONFIG_INVALID", false, None);
}

// Row: Both server key variables set.
#[test]
fn secret_key_conflict() {
    let path = std::path::Path::new("/run/secrets/k");
    let result = crate::server_secret_backend(None, Some(path), Some(path));
    assert_error(result, "CRED_SECRET_KEY_CONFLICT", false, None);
}

// Row: No session bus and no key variable.
#[test]
fn secret_key_missing() {
    assert_error(
        crate::server_secret_backend(None, None, None),
        "CRED_SECRET_KEY_MISSING",
        false,
        None,
    );
    assert_error(
        crate::server_secret_backend(Some(""), None, None),
        "CRED_SECRET_KEY_MISSING",
        false,
        None,
    );
}

// Row: Key bytes not 32 bytes / base64 of 32.
#[tokio::test]
async fn secret_key_invalid() {
    let dir = temp_dir();
    let key = dir.join("short.key");
    std::fs::write(&key, b"only-ten-b").unwrap();
    let backend = SecretBackendConfig::EncryptedFile {
        key_source: KeySource::KeyFile(key),
    };
    let pool = migrated_pool(&dir).await;
    let result =
        IdentityService::new(config(Program::PersonalServer, backend, &dir), Some(pool)).await;
    assert_error(result.map(|_| ()), "CRED_SECRET_KEY_INVALID", false, None);
}

// Row: `systemd-creds decrypt --user` failed or timed out.
#[tokio::test]
async fn systemd_creds_failed() {
    let dir = temp_dir();
    let credential = dir.join("secrets.key.cred");
    std::fs::write(&credential, b"not a credential this user encrypted").unwrap();
    let backend = SecretBackendConfig::EncryptedFile {
        key_source: KeySource::SystemdCredential(credential),
    };
    let pool = migrated_pool(&dir).await;
    let result =
        IdentityService::new(config(Program::PersonalServer, backend, &dir), Some(pool)).await;
    assert_error(result.map(|_| ()), "CRED_SYSTEMD_CREDS_FAILED", false, None);
}

// Row: Secrets file cannot be decrypted or unknown format.
#[tokio::test]
async fn secret_store_corrupt() {
    let dir = temp_dir();
    std::fs::create_dir_all(dir.join("secrets")).unwrap();
    std::fs::write(
        dir.join("secrets/secrets.enc"),
        br#"{"format":2,"nonce":"","ciphertext":""}"#,
    )
    .unwrap();
    let backend = SecretBackendConfig::EncryptedFile {
        key_source: KeySource::KeyFile(key_file(&dir, [1u8; 32])),
    };
    let pool = migrated_pool(&dir).await;
    let result =
        IdentityService::new(config(Program::PersonalServer, backend, &dir), Some(pool)).await;
    assert_error(result.map(|_| ()), "CRED_SECRET_STORE_CORRUPT", false, None);
}

// Row: Secrets file write failed.
#[cfg(unix)]
#[tokio::test]
async fn secret_store_write_failed() {
    use std::os::unix::fs::PermissionsExt as _;
    let h = Harness::with(Setup {
        program: Program::PersonalServer,
        store: Store::File,
        browser: false,
        ..Setup::default()
    })
    .await;
    let name = SecretName::new("telegram/bot-token").unwrap();
    h.service.secrets().put(&name, b"first").await.unwrap();
    let secrets_dir = h.dir.join("secrets");
    std::fs::set_permissions(&secrets_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let result = h.service.secrets().put(&name, b"second").await;
    std::fs::set_permissions(&secrets_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_error(
        result,
        "CRED_SECRET_STORE_WRITE_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
    // The previous value stays.
    assert_eq!(
        h.service
            .secrets()
            .get(&name)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        b"first"
    );
}

// Row: Scaled shape, secret store called.
#[tokio::test]
async fn secret_store_absent() {
    let dir = temp_dir();
    let service = IdentityService::new(
        config(Program::AgentScaledShape, SecretBackendConfig::None, &dir),
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let name = SecretName::new("nats/credentials").unwrap();
    assert_error(
        service.secrets().put(&name, b"x").await,
        "CRED_SECRET_STORE_ABSENT",
        false,
        None,
    );
    assert_error(
        service.secrets().get(&name).await,
        "CRED_SECRET_STORE_ABSENT",
        false,
        None,
    );
}

// Row: Secret name fails pattern.
#[test]
fn secret_name_invalid() {
    for bad in ["", "Upper/case", "-leading", "has space", &"a".repeat(201)] {
        assert_error(
            SecretName::new(bad),
            "CRED_SECRET_NAME_INVALID",
            false,
            None,
        );
    }
    assert!(SecretName::new(&"a".repeat(200)).is_ok());
}

// Row: Secret value > 64 KiB.
#[tokio::test]
async fn secret_too_large() {
    let h = Harness::new().await;
    let name = SecretName::new("telegram/bot-token").unwrap();
    let big = vec![0u8; 64 * 1024 + 1];
    assert_error(
        h.service.secrets().put(&name, &big).await,
        "CRED_SECRET_TOO_LARGE",
        false,
        None,
    );
    assert!(h
        .service
        .secrets()
        .put(&name, &big[..64 * 1024])
        .await
        .is_ok());
}

// Row: Keychain or Secret Service locked, refused or unreachable.
#[tokio::test]
async fn keychain_unavailable() {
    let h = Harness::new().await;
    h.keychain.locked.store(true, Ordering::SeqCst);
    let name = SecretName::new("telegram/bot-token").unwrap();
    assert_error(
        h.service.secrets().get(&name).await,
        "CRED_KEYCHAIN_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Row: Secret item missing for an active credential.
#[tokio::test]
async fn keychain_missing() {
    let h = Harness::new().await;
    let id = h
        .seed(CredentialKind::Github, "asharao", Seed::default())
        .await;
    h.keychain.items.lock().unwrap().clear();
    assert_error(
        h.service.access_token(CredentialKind::Github, None).await,
        "CRED_KEYCHAIN_MISSING",
        false,
        Some(RepairAction::SignInAgain),
    );
    // The credential is now marked, so the next call needs a sign-in.
    assert_eq!(
        code_of(h.service.access_token_by_id(id).await),
        "CRED_SIGN_IN_REQUIRED"
    );
}

// Row: Issuer or site not `https` / too long.
#[tokio::test]
async fn url_invalid() {
    let h = Harness::new().await;
    let oidc = ProviderConfig::Oidc {
        issuer: "http://example.okta.com".to_owned(),
        client_id: "0oa1b2c3".to_owned(),
        scopes: vec![],
    };
    assert_error(
        h.service
            .begin_sign_in(request(oidc, SignInMethod::Browser))
            .await,
        "CRED_URL_INVALID",
        false,
        Some(RepairAction::SendToIt),
    );
    let long = format!("https://example.com/{}", "a".repeat(2048));
    let confluence = ProviderConfig::Confluence {
        site: long,
        client_id: "atl-123".to_owned(),
    };
    assert_eq!(
        code_of(
            h.service
                .begin_sign_in(request(confluence, SignInMethod::Browser))
                .await
        ),
        "CRED_URL_INVALID"
    );
}

// Row: Client id or GitHub App id absent.
#[tokio::test]
async fn provider_not_configured() {
    let h = Harness::with(Setup {
        github: false,
        ..Setup::default()
    })
    .await;
    let github = ProviderConfig::Github {
        host: "github.com".to_owned(),
    };
    assert_error(
        h.service
            .begin_sign_in(request(github, SignInMethod::Browser))
            .await,
        "CRED_PROVIDER_NOT_CONFIGURED",
        false,
        Some(RepairAction::SendToIt),
    );
}

// Row: Flow not offered here.
#[tokio::test]
async fn flow_not_offered() {
    let h = Harness::with(Setup {
        program: Program::PersonalServer,
        browser: false,
        ..Setup::default()
    })
    .await;
    let confluence = ProviderConfig::Confluence {
        site: "https://acme.atlassian.net".to_owned(),
        client_id: "atl-123".to_owned(),
    };
    assert_error(
        h.service
            .begin_sign_in(request(confluence, SignInMethod::DeviceCode))
            .await,
        "CRED_FLOW_NOT_OFFERED",
        false,
        None,
    );
    // No browser on the server; Slack by device code.
    assert_eq!(
        code_of(
            h.service
                .begin_sign_in(request(microsoft(), SignInMethod::Browser))
                .await
        ),
        "CRED_FLOW_NOT_OFFERED"
    );
    assert_eq!(
        code_of(
            h.service
                .begin_sign_in(request(slack(), SignInMethod::DeviceCode))
                .await
        ),
        "CRED_FLOW_NOT_OFFERED"
    );
    // The agent profile: no person signs in.
    let agent = Harness::with(Setup {
        program: Program::AgentSingleShape,
        browser: false,
        ..Setup::default()
    })
    .await;
    assert_eq!(
        code_of(
            agent
                .service
                .begin_sign_in(request(microsoft(), SignInMethod::DeviceCode))
                .await
        ),
        "CRED_FLOW_NOT_OFFERED"
    );
}

// Row: All loopback ports busy.
#[tokio::test]
async fn loopback_busy() {
    let _ports = PORTS.lock().await;
    let h = Harness::new().await;
    let held: Vec<_> = futures_bind().await;
    assert_error(
        h.service
            .begin_sign_in(request(microsoft(), SignInMethod::Browser))
            .await,
        "CRED_LOOPBACK_BUSY",
        true,
        Some(RepairAction::TryAgain),
    );
    drop(held);
}

async fn futures_bind() -> Vec<tokio::net::TcpListener> {
    let mut held = Vec::new();
    for port in crate::loopback::PORTS {
        if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            held.push(l);
        }
    }
    held
}

// Row: System browser could not be opened.
#[tokio::test]
async fn browser_failed() {
    let _ports = PORTS.lock().await;
    let h = Harness::new().await;
    h.browser.fail.store(true, Ordering::SeqCst);
    assert_error(
        h.service
            .begin_sign_in(request(microsoft(), SignInMethod::Browser))
            .await,
        "CRED_BROWSER_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Row: More than 4 sign-ins pending.
#[tokio::test]
async fn too_many_sign_ins() {
    let h = Harness::new().await;
    github_device_mock(&h, 600).await;
    let github = || ProviderConfig::Github {
        host: "github.com".to_owned(),
    };
    let mut ids = Vec::new();
    for _ in 0..4 {
        match h
            .service
            .begin_sign_in(request(github(), SignInMethod::DeviceCode))
            .await
        {
            Ok(SignInChallenge::DeviceCode { challenge_id, .. }) => ids.push(challenge_id),
            other => panic!("{:?}", other.map(|_| ()).map_err(|e| e.to_string())),
        }
    }
    assert_error(
        h.service
            .begin_sign_in(request(github(), SignInMethod::DeviceCode))
            .await,
        "CRED_TOO_MANY_SIGN_INS",
        true,
        Some(RepairAction::TryAgain),
    );
    for id in ids {
        h.service.cancel_sign_in(id).unwrap();
    }
}

// Row: Unknown challenge id.
#[tokio::test]
async fn challenge_unknown() {
    let h = Harness::new().await;
    let id = crate::types::SignInChallengeId(uuid::Uuid::now_v7());
    assert_error(
        h.service.wait_for_sign_in(id).await,
        "CRED_CHALLENGE_UNKNOWN",
        false,
        None,
    );
    assert_error(
        h.service.cancel_sign_in(id),
        "CRED_CHALLENGE_UNKNOWN",
        false,
        None,
    );
}

// Row: Person cancelled or denied at the provider.
#[tokio::test]
async fn sign_in_cancelled() {
    let h = Harness::new().await;
    github_device_mock(&h, 600).await;
    let github = ProviderConfig::Github {
        host: "github.com".to_owned(),
    };
    let Ok(SignInChallenge::DeviceCode { challenge_id, .. }) = h
        .service
        .begin_sign_in(request(github, SignInMethod::DeviceCode))
        .await
    else {
        panic!("device challenge");
    };
    h.service.cancel_sign_in(challenge_id).unwrap();
    assert_error(
        h.service.wait_for_sign_in(challenge_id).await,
        "CRED_SIGN_IN_CANCELLED",
        false,
        Some(RepairAction::SignInAgain),
    );
}

// Row: Sign-in not finished in time (10 min / device expiry).
#[tokio::test]
async fn sign_in_timeout() {
    let h = Harness::new().await;
    github_device_mock(&h, 1).await;
    let github = ProviderConfig::Github {
        host: "github.com".to_owned(),
    };
    let Ok(SignInChallenge::DeviceCode { challenge_id, .. }) = h
        .service
        .begin_sign_in(request(github, SignInMethod::DeviceCode))
        .await
    else {
        panic!("device challenge");
    };
    assert_error(
        h.service.wait_for_sign_in(challenge_id).await,
        "CRED_SIGN_IN_TIMEOUT",
        true,
        Some(RepairAction::SignInAgain),
    );
}

// Row: Relayed sign-in's `state` differs from the challenge's.
#[tokio::test]
async fn relay_state_mismatch() {
    let h = Harness::with(Setup {
        program: Program::PersonalServer,
        browser: false,
        ..Setup::default()
    })
    .await;
    let mut req = request(slack(), SignInMethod::RelayedBrowser);
    req.relay_port = Some(47821);
    let Ok(SignInChallenge::RelayedBrowser { challenge_id, .. }) =
        h.service.begin_sign_in(req).await
    else {
        panic!("relayed challenge");
    };
    assert_error(
        h.service
            .complete_relayed_sign_in(
                challenge_id,
                SecretString::new("code".into()),
                "wrong".into(),
            )
            .await,
        "CRED_RELAY_STATE_MISMATCH",
        false,
        Some(RepairAction::SignInAgain),
    );
}

// Row: Provider refused the client or the grant (incl. C-12/C-14 failing).
#[tokio::test]
async fn provider_refused() {
    let _ports = PORTS.lock().await;
    let h = Harness::new().await;
    Mock::given(method("POST"))
        .and(path(
            "/login.microsoftonline.com/4a1c-tenant/oauth2/v2.0/token",
        ))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_client"})))
        .mount(&h.mock)
        .await;
    let Ok(SignInChallenge::BrowserOpened { challenge_id, .. }) = h
        .service
        .begin_sign_in(request(microsoft(), SignInMethod::Browser))
        .await
    else {
        panic!("browser challenge");
    };
    let url = h.browser.last().unwrap();
    let state = super::harness::query(&url)["state"].clone();
    assert_eq!(
        super::harness::redirect(&url, &[("code", "c-1"), ("state", &state)]).await,
        400
    );
    assert_error(
        h.service.wait_for_sign_in(challenge_id).await,
        "CRED_PROVIDER_REFUSED",
        false,
        Some(RepairAction::SendToIt),
    );
}

// Row: Provider unreachable (DNS, TLS, timeout, 5xx).
#[tokio::test]
async fn provider_unreachable() {
    let h = Harness::new().await;
    Mock::given(method("GET"))
        .and(path("/example.okta.com/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&h.mock)
        .await;
    let oidc = ProviderConfig::Oidc {
        issuer: "https://example.okta.com".to_owned(),
        client_id: "0oa1b2c3".to_owned(),
        scopes: vec![],
    };
    assert_error(
        h.service
            .begin_sign_in(request(oidc, SignInMethod::Browser))
            .await,
        "CRED_PROVIDER_UNREACHABLE",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Row: Account label out of range.
#[tokio::test]
async fn label_invalid() {
    let h = Harness::new().await;
    let result = h
        .service
        .save_mail_password(
            "",
            MailPasswordKind::AppPassword,
            SecretString::new("pw".into()),
        )
        .await;
    assert_error(
        result,
        "CRED_LABEL_INVALID",
        false,
        Some(RepairAction::SendToIt),
    );
    let long = "a".repeat(255);
    let result = h
        .service
        .save_mail_password(
            &long,
            MailPasswordKind::AppPassword,
            SecretString::new("pw".into()),
        )
        .await;
    assert_eq!(code_of(result), "CRED_LABEL_INVALID");
}

// Row: Active credential with same kind and label exists.
#[tokio::test]
async fn duplicate() {
    let h = Harness::new().await;
    let save = |site: &'static str| {
        h.service.save_jira_token(
            site,
            "sam.lee@acme.co.uk",
            SecretString::new("tok".into()),
            "5b10",
            "Sam Lee",
        )
    };
    save("acme.atlassian.net").await.unwrap();
    assert_error(
        save("other.atlassian.net").await,
        "CRED_DUPLICATE",
        false,
        None,
    );
}

// Row: Database write failed.
#[tokio::test]
async fn store_failed() {
    let h = Harness::new().await;
    sqlx::raw_sql("DROP TABLE credential_meta")
        .execute(&h.pool)
        .await
        .unwrap();
    let result = h
        .service
        .save_mail_password(
            "asha@acme.com",
            MailPasswordKind::MailPassword,
            SecretString::new("pw-1".into()),
        )
        .await;
    assert_error(
        result,
        "CRED_STORE_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
    // The secret written first was removed again (no orphan).
    assert!(h.keychain.items.lock().unwrap().is_empty());
}

// Row: No usable credential / marked needs sign-in / refresh refused.
#[tokio::test]
async fn sign_in_required() {
    let h = Harness::new().await;
    assert_error(
        h.service.access_token(CredentialKind::Slack, None).await,
        "CRED_SIGN_IN_REQUIRED",
        false,
        Some(RepairAction::SignInAgain),
    );
}

// Row: Provider refused the refresh — recorded as `last_error_code`, not returned.
#[tokio::test]
async fn refresh_refused() {
    let h = Harness::new().await;
    Mock::given(method("POST"))
        .and(path("/provider.example/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
        .mount(&h.mock)
        .await;
    h.seed(
        CredentialKind::Microsoft,
        "asha.rao@acme.com",
        Seed {
            expires_at: Some(in_minutes(2)),
            ..Seed::default()
        },
    )
    .await;
    assert_eq!(
        code_of(
            h.service
                .access_token(CredentialKind::Microsoft, None)
                .await
        ),
        "CRED_SIGN_IN_REQUIRED"
    );
    let stored: String = sqlx::query_scalar("SELECT last_error_code FROM credential_meta")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(stored, "CRED_REFRESH_REFUSED");
    let error = CredError::of("CRED_REFRESH_REFUSED");
    assert!(!error.retryable());
    assert_eq!(error.repair(), None);
}

// Row: Several active credentials, none named.
#[tokio::test]
async fn ambiguous() {
    let h = Harness::new().await;
    h.seed(CredentialKind::Github, "asharao", Seed::default())
        .await;
    h.seed(CredentialKind::Github, "asha-work", Seed::default())
        .await;
    assert_error(
        h.service.access_token(CredentialKind::Github, None).await,
        "CRED_AMBIGUOUS",
        false,
        None,
    );
    assert!(h
        .service
        .access_token(CredentialKind::Github, Some("asharao"))
        .await
        .is_ok());
}

// Row: `access_token` on a password kind.
#[tokio::test]
async fn kind_has_no_token() {
    let h = Harness::new().await;
    let id = h
        .service
        .save_mail_password(
            "asha@acme.com",
            MailPasswordKind::MailPassword,
            SecretString::new("pw-1".into()),
        )
        .await
        .unwrap()
        .id;
    assert_error(
        h.service
            .access_token(CredentialKind::MailServerPassword, None)
            .await,
        "CRED_KIND_HAS_NO_TOKEN",
        false,
        None,
    );
    assert_eq!(
        code_of(h.service.access_token_by_id(id).await),
        "CRED_KIND_HAS_NO_TOKEN"
    );
}

// Row: `mail_password` on a token kind.
#[tokio::test]
async fn kind_mismatch() {
    let h = Harness::new().await;
    let id = h
        .seed(CredentialKind::Github, "asharao", Seed::default())
        .await;
    assert_error(
        h.service.mail_password(id).await,
        "CRED_KIND_MISMATCH",
        false,
        None,
    );
}

// Row: Unknown credential id.
#[tokio::test]
async fn not_found() {
    let h = Harness::new().await;
    assert_error(
        h.service.sign_out(CredentialId::new()).await,
        "CRED_NOT_FOUND",
        false,
        None,
    );
    assert_eq!(
        code_of(h.service.mail_password(CredentialId::new()).await),
        "CRED_NOT_FOUND"
    );
}

// Row: Password empty or > 1024 bytes.
#[tokio::test]
async fn password_invalid() {
    let h = Harness::new().await;
    for bad in [String::new(), "x".repeat(1025)] {
        let result = h
            .service
            .save_mail_password(
                "asha@acme.com",
                MailPasswordKind::AppPassword,
                SecretString::new(bad),
            )
            .await;
        assert_error(result, "CRED_PASSWORD_INVALID", false, None);
    }
}

// Row: A secret found in a record field (test-time guard).
#[tokio::test]
async fn secret_in_record() {
    let h = Harness::new().await;
    let result = h
        .service
        .save_mail_password(
            "asha@acme.com",
            MailPasswordKind::AppPassword,
            SecretString::new("asha@acme.com".into()),
        )
        .await;
    assert_error(result, "CRED_SECRET_IN_RECORD", false, None);
    assert_eq!(h.count("SELECT count(*) FROM credentials").await, 0);
}

// Row: Active credential lacks a scope the caller needs.
#[tokio::test]
async fn scope_missing() {
    let h = Harness::new().await;
    h.seed(
        CredentialKind::Microsoft,
        "asha.rao@acme.com",
        Seed {
            scopes: &["https://graph.microsoft.com/Files.ReadWrite.All"],
            ..Seed::default()
        },
    )
    .await;
    assert_error(
        h.service
            .access_token_for(
                CredentialKind::Microsoft,
                &["https://graph.microsoft.com/Sites.Read.All"],
            )
            .await,
        "CRED_SCOPE_MISSING",
        false,
        Some(RepairAction::SignInAgain),
    );
}

const CERT: &[u8] = include_bytes!("../../tests/fixtures/acme-desktop.cert.pem");
const OTHER_KEY: &[u8] = include_bytes!("../../tests/fixtures/other.key.pem");
const EXPIRED_CERT: &[u8] = include_bytes!("../../tests/fixtures/expired.cert.pem");
const EXPIRED_KEY: &[u8] = include_bytes!("../../tests/fixtures/expired.key.pem");

// Row: Imported client certificate or key unreadable, missing, unloadable or not a pair.
#[tokio::test]
async fn certificate_invalid() {
    let h = Harness::new().await;
    let result = h
        .service
        .import_client_certificate(
            "ACME desktop",
            CERT,
            zeroize::Zeroizing::new(OTHER_KEY.to_vec()),
        )
        .await;
    assert_error(result, "CRED_CERTIFICATE_INVALID", false, None);
    let result = h
        .service
        .import_client_certificate(
            "ACME desktop",
            b"no pem here",
            zeroize::Zeroizing::new(OTHER_KEY.to_vec()),
        )
        .await;
    assert_eq!(code_of(result), "CRED_CERTIFICATE_INVALID");
}

// Row: Imported client certificate already expired.
#[tokio::test]
async fn certificate_expired() {
    let h = Harness::new().await;
    let result = h
        .service
        .import_client_certificate(
            "ACME desktop",
            EXPIRED_CERT,
            zeroize::Zeroizing::new(EXPIRED_KEY.to_vec()),
        )
        .await;
    assert_error(
        result,
        "CRED_CERTIFICATE_EXPIRED",
        false,
        Some(RepairAction::SendToIt),
    );
}

// Row: `create_secrets_key_credential` target file already exists.
#[tokio::test]
async fn secret_key_exists() {
    let dir = temp_dir();
    let path = dir.join("secrets.key.cred");
    std::fs::write(&path, b"existing").unwrap();
    assert_error(
        IdentityService::create_secrets_key_credential(&path).await,
        "CRED_SECRET_KEY_EXISTS",
        false,
        None,
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"existing");
}
