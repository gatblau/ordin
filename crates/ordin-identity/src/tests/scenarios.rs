//! X2: one test per Gherkin scenario of the Acceptance Criteria and of both amendments, named after
//! the scenario (exact title in the comment above each).

use std::sync::atomic::Ordering;
use std::time::Duration;

use ordin_types::domain::identity::{CredentialKind, CredentialState};
use ordin_types::CodedError as _;
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, ResponseTemplate};

use super::harness::{
    code_of, id_token, in_minutes, query, redirect, Capture, Harness, Seed, Setup, PORTS,
};
use crate::config::Program;
use crate::secret::{SecretName, SecretString};
use crate::types::{
    DesktopKeyStore, JiraCredentialMethod, MailPasswordKind, ProviderConfig, ServerSecrets,
    SignInChallenge, SignInMethod, SignInRequest, SignInState,
};

const MS_TOKEN: &str = "/login.microsoftonline.com/4a1c-tenant/oauth2/v2.0/token";
const FILES: &str = "https://graph.microsoft.com/Files.ReadWrite.All";
const SITES: &str = "https://graph.microsoft.com/Sites.Read.All";
const MAIL: &str = "Mail.ReadWrite";
const SLACK_FIRST: [&str; 5] = [
    "chat:write",
    "channels:history",
    "groups:history",
    "channels:read",
    "groups:read",
];

fn microsoft() -> ProviderConfig {
    ProviderConfig::Microsoft {
        tenant_id: "4a1c-tenant".to_owned(),
        client_id: "9f0e-app".to_owned(),
    }
}

fn sign_in(provider: ProviderConfig, scopes: &[&str], method: SignInMethod) -> SignInRequest {
    SignInRequest {
        provider,
        scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        method,
        relay_port: None,
        reason: Some("SharePoint".to_owned()),
    }
}

async fn microsoft_token_mock(h: &Harness, scope: &str) {
    Mock::given(method("POST"))
        .and(path(MS_TOKEN))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "ms-access-0001", "refresh_token": "ms-refresh-0001", "expires_in": 3600,
            "scope": scope, "token_type": "Bearer",
            "id_token": id_token(json!({"preferred_username": "asha.rao@acme.com", "name": "Asha Rao"}))
        })))
        .mount(&h.mock)
        .await;
}

fn challenge_id(challenge: &SignInChallenge) -> crate::types::SignInChallengeId {
    match challenge {
        SignInChallenge::BrowserOpened { challenge_id, .. }
        | SignInChallenge::DeviceCode { challenge_id, .. }
        | SignInChallenge::RelayedBrowser { challenge_id, .. } => *challenge_id,
        SignInChallenge::AlreadySignedIn { .. } => panic!("already signed in"),
    }
}

/// Completes a browser sign-in by playing the provider's redirect with a code and the right state.
async fn complete_browser(
    h: &Harness,
    challenge: &SignInChallenge,
) -> ordin_types::domain::identity::Credential {
    let url = h.browser.last().expect("browser opened");
    let state = query(&url)["state"].clone();
    assert_eq!(
        redirect(&url, &[("code", "code-1"), ("state", &state)]).await,
        200
    );
    h.service
        .wait_for_sign_in(challenge_id(challenge))
        .await
        .expect("signed in")
}

// Scenario: Happy path — Microsoft sign-in on the desktop
#[tokio::test]
async fn happy_path_microsoft_sign_in_on_the_desktop() {
    let _ports = PORTS.lock().await;
    let h = Harness::new().await;
    microsoft_token_mock(&h, &format!("{FILES} {SITES} offline_access")).await;
    let challenge = h
        .service
        .begin_sign_in(sign_in(microsoft(), &[FILES, SITES], SignInMethod::Browser))
        .await
        .unwrap();
    let url = h.browser.last().unwrap();
    assert!(url.starts_with("https://login.microsoftonline.com/4a1c-tenant/oauth2/v2.0/authorize?"));
    let q = query(&url);
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["code_challenge"].len(), 43);
    assert_eq!(q["redirect_uri"], "http://127.0.0.1:47821/callback");
    let credential = complete_browser(&h, &challenge).await;
    assert_eq!(credential.kind, CredentialKind::Microsoft);
    assert_eq!(credential.state, CredentialState::Active);
    assert_eq!(
        credential.keychain_ref,
        format!("credential/microsoft/{}", credential.id)
    );
    let blob = h
        .blob(CredentialKind::Microsoft, credential.id)
        .await
        .unwrap();
    assert_eq!(blob["access_token"], "ms-access-0001");
    let tables = h.table_text().await;
    assert!(!tables.contains("ms-access-0001") && !tables.contains("ms-refresh-0001"));
}

// Scenario: Edge — scopes already granted
#[tokio::test]
async fn edge_scopes_already_granted() {
    let h = Harness::new().await;
    h.seed(
        CredentialKind::Microsoft,
        "asha.rao@acme.com",
        Seed {
            scopes: &[FILES, SITES],
            ..Seed::default()
        },
    )
    .await;
    let challenge = h
        .service
        .begin_sign_in(sign_in(microsoft(), &[FILES], SignInMethod::Browser))
        .await
        .unwrap();
    assert!(matches!(challenge, SignInChallenge::AlreadySignedIn { .. }));
    assert!(h.browser.last().is_none());
}

// Scenario: Edge — incremental consent adds mail permissions
#[tokio::test]
async fn edge_incremental_consent_adds_mail_permissions() {
    let _ports = PORTS.lock().await;
    let h = Harness::new().await;
    let id = h
        .seed(
            CredentialKind::Microsoft,
            "asha.rao@acme.com",
            Seed {
                scopes: &[FILES],
                ..Seed::default()
            },
        )
        .await;
    microsoft_token_mock(&h, &format!("{FILES} {MAIL} offline_access")).await;
    let challenge = h
        .service
        .begin_sign_in(sign_in(microsoft(), &[MAIL], SignInMethod::Browser))
        .await
        .unwrap();
    let asked = query(&h.browser.last().unwrap())["scope"].clone();
    assert!(asked.contains(FILES) && asked.contains(MAIL));
    let credential = complete_browser(&h, &challenge).await;
    assert_eq!(credential.id, id, "the same row is updated");
    assert!(credential.refreshed_at.is_some());
    assert_eq!(h.count("SELECT count(*) FROM credentials").await, 1);
    let scopes: String = sqlx::query_scalar("SELECT scopes FROM credential_meta")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert!(scopes.contains(FILES) && scopes.contains(MAIL));
}

// Scenario: Happy path — device sign-in on the server
#[tokio::test]
async fn happy_path_device_sign_in_on_the_server() {
    let h = Harness::with(Setup {
        program: Program::PersonalServer,
        browser: false,
        ..Setup::default()
    })
    .await;
    Mock::given(method("POST"))
        .and(path("/github.com/login/device/code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "dc-1", "user_code": "WDJB-MJHT",
            "verification_uri": "https://github.com/login/device", "expires_in": 900, "interval": 1
        })))
        .mount(&h.mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/github.com/login/oauth/access_token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"error": "authorization_pending"})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&h.mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/github.com/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "ghu_0001", "refresh_token": "ghr_0001", "expires_in": 28800, "token_type": "bearer"
        })))
        .with_priority(2)
        .mount(&h.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api.github.com/user"))
        .and(header("authorization", "Bearer ghu_0001"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"login": "asharao", "name": "Asha Rao"})),
        )
        .mount(&h.mock)
        .await;
    let github = ProviderConfig::Github {
        host: "github.com".to_owned(),
    };
    let challenge = h
        .service
        .begin_sign_in(sign_in(github, &[], SignInMethod::DeviceCode))
        .await
        .unwrap();
    let SignInChallenge::DeviceCode {
        verification_uri,
        user_code,
        ..
    } = &challenge
    else {
        panic!("device challenge");
    };
    assert_eq!(verification_uri, "https://github.com/login/device");
    assert_eq!(user_code, "WDJB-MJHT");
    let started = std::time::Instant::now();
    let credential = h
        .service
        .wait_for_sign_in(challenge_id(&challenge))
        .await
        .unwrap();
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "two polls one interval apart"
    );
    assert_eq!(credential.account_label, "asharao");
}

// Scenario: Error — Confluence on the server
#[tokio::test]
async fn error_confluence_on_the_server() {
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
    let result = h
        .service
        .begin_sign_in(sign_in(confluence, &[], SignInMethod::DeviceCode))
        .await;
    assert_eq!(code_of(result), "CRED_FLOW_NOT_OFFERED");
}

// Scenario: Edge — token refreshed before expiry
#[tokio::test]
async fn edge_token_refreshed_before_expiry() {
    let h = Harness::new().await;
    Mock::given(method("POST"))
        .and(path("/github.com/login/oauth/access_token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token": "ghu_new", "refresh_token": "ghr_new", "expires_in": 28800}))
                .set_delay(Duration::from_millis(200)),
        )
        .expect(1)
        .mount(&h.mock)
        .await;
    h.seed(
        CredentialKind::Github,
        "asharao",
        Seed {
            expires_at: Some(in_minutes(3)),
            token_endpoint: "https://github.com/login/oauth/access_token",
            ..Seed::default()
        },
    )
    .await;
    let (a, b) = tokio::join!(
        h.service.access_token(CredentialKind::Github, None),
        h.service.access_token(CredentialKind::Github, None)
    );
    assert_eq!(a.unwrap().token.expose(), "ghu_new");
    assert_eq!(b.unwrap().token.expose(), "ghu_new");
    h.mock.verify().await;
}

// Scenario: Error — refresh refused
#[tokio::test]
async fn error_refresh_refused() {
    let h = Harness::new().await;
    Mock::given(method("POST"))
        .and(path("/slack.com/api/oauth.v2.access"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": false, "error": "invalid_grant"})),
        )
        .mount(&h.mock)
        .await;
    h.seed(
        CredentialKind::Slack,
        "Acme/U0123",
        Seed {
            expires_at: Some(in_minutes(1)),
            token_endpoint: "https://slack.com/api/oauth.v2.access",
            ..Seed::default()
        },
    )
    .await;
    let mut events = h.service.subscribe();
    assert_eq!(
        code_of(h.service.access_token(CredentialKind::Slack, None).await),
        "CRED_SIGN_IN_REQUIRED"
    );
    let event = events.try_recv().unwrap();
    assert_eq!(event.state, SignInState::NeedsSignIn);
}

// Scenario: Error — refused mail password is never retried
#[tokio::test]
async fn error_refused_mail_password_is_never_retried() {
    let h = Harness::new().await;
    let pw = |s: &str| SecretString::new(s.to_owned());
    let credential = h
        .service
        .save_mail_password(
            "asha@acme.com",
            MailPasswordKind::AppPassword,
            pw("app-pw-1"),
        )
        .await
        .unwrap();
    h.service
        .report_refused(credential.id, "MAIL_AUTH_FAILED")
        .await
        .unwrap();
    let gets = h.keychain.gets.load(Ordering::SeqCst);
    assert_eq!(
        code_of(h.service.mail_password(credential.id).await),
        "CRED_SIGN_IN_REQUIRED"
    );
    assert_eq!(
        h.keychain.gets.load(Ordering::SeqCst),
        gets,
        "the keychain was not read"
    );
    h.service
        .save_mail_password(
            "asha@acme.com",
            MailPasswordKind::AppPassword,
            pw("app-pw-2"),
        )
        .await
        .unwrap();
    assert_eq!(
        h.service
            .mail_password(credential.id)
            .await
            .unwrap()
            .expose(),
        "app-pw-2"
    );
}

// Scenario: Error — person cancels at the provider
#[tokio::test]
async fn error_person_cancels_at_the_provider() {
    let _ports = PORTS.lock().await;
    let h = Harness::new().await;
    let challenge = h
        .service
        .begin_sign_in(sign_in(microsoft(), &[FILES], SignInMethod::Browser))
        .await
        .unwrap();
    let url = h.browser.last().unwrap();
    let state = query(&url)["state"].clone();
    // A redirect with the wrong state is refused and the flow stays open.
    assert_eq!(
        redirect(&url, &[("code", "c"), ("state", "wrong")]).await,
        400
    );
    redirect(&url, &[("error", "access_denied"), ("state", &state)]).await;
    assert_eq!(
        code_of(h.service.wait_for_sign_in(challenge_id(&challenge)).await),
        "CRED_SIGN_IN_CANCELLED"
    );
    assert_eq!(h.count("SELECT count(*) FROM credentials").await, 0);
}

// Scenario: Error — provider unreachable
#[tokio::test]
async fn error_provider_unreachable() {
    let h = Harness::new().await;
    Mock::given(method("GET"))
        .and(path("/example.okta.com/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&h.mock)
        .await;
    let oidc = ProviderConfig::Oidc {
        issuer: "https://example.okta.com".to_owned(),
        client_id: "0oa1b2c3".to_owned(),
        scopes: vec!["openid".to_owned()],
    };
    // The harness's per-call timeout stands for the 15 seconds.
    let result = h
        .service
        .begin_sign_in(sign_in(oidc, &[], SignInMethod::Browser))
        .await;
    let Err(error) = result else {
        panic!("expected a failure")
    };
    assert_eq!(error.code(), "CRED_PROVIDER_UNREACHABLE");
    assert!(error.retryable());
}

// Scenario: Error — keychain locked
#[tokio::test]
async fn error_keychain_locked() {
    let h = Harness::new().await;
    h.seed(CredentialKind::Github, "asharao", Seed::default())
        .await;
    h.keychain.locked.store(true, Ordering::SeqCst);
    let Err(error) = h.service.access_token(CredentialKind::Github, None).await else {
        panic!("expected a failure");
    };
    assert_eq!(error.code(), "CRED_KEYCHAIN_UNAVAILABLE");
    assert!(error.retryable());
}

// Scenario: Error — a token without the needed scope
#[tokio::test]
async fn error_a_token_without_the_needed_scope() {
    let h = Harness::new().await;
    h.seed(
        CredentialKind::Microsoft,
        "asha.rao@acme.com",
        Seed {
            scopes: &[FILES],
            ..Seed::default()
        },
    )
    .await;
    let result = h
        .service
        .access_token_for(CredentialKind::Microsoft, &[SITES])
        .await;
    assert_eq!(code_of(result), "CRED_SCOPE_MISSING");
    assert!(
        h.mock.received_requests().await.unwrap().is_empty(),
        "no provider request"
    );
}

// Scenario: Happy path — a company token for one hosted agent's audience
#[tokio::test]
async fn happy_path_a_company_token_for_one_hosted_agent_s_audience() {
    let h = Harness::new().await;
    Mock::given(method("POST"))
        .and(path("/example.okta.com/oauth2/v1/token"))
        .and(body_string_contains("audience=api%3A%2F%2Ffinance-agent"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token": "aud-1", "expires_in": 3600})),
        )
        .expect(2)
        .mount(&h.mock)
        .await;
    h.seed(
        CredentialKind::Oidc,
        "asha.rao@acme.com",
        Seed {
            client_id: "0oa1b2c3",
            provider_host: "https://example.okta.com",
            token_endpoint: "https://example.okta.com/oauth2/v1/token",
            ..Seed::default()
        },
    )
    .await;
    let ask = || {
        h.service.access_token_for_audience(
            "https://example.okta.com",
            "0oa1b2c3",
            "api://finance-agent",
        )
    };
    assert_eq!(ask().await.unwrap().token.expose(), "aud-1");
    assert_eq!(ask().await.unwrap().token.expose(), "aud-1");
    h.service
        .invalidate_audience_token(
            "https://example.okta.com",
            "0oa1b2c3",
            "api://finance-agent",
        )
        .await;
    ask().await.unwrap();
    h.mock.verify().await;
}

// Scenario: Edge — silent refresh never prompts
#[tokio::test]
async fn edge_silent_refresh_never_prompts() {
    let h = Harness::new().await;
    Mock::given(method("POST"))
        .and(path("/github.com/login/oauth/access_token"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"error": "bad_refresh_token"})),
        )
        .mount(&h.mock)
        .await;
    h.seed(
        CredentialKind::Github,
        "asharao",
        Seed {
            token_endpoint: "https://github.com/login/oauth/access_token",
            ..Seed::default()
        },
    )
    .await;
    let mut events = h.service.subscribe();
    assert!(!h
        .service
        .refresh_silently(CredentialKind::Github)
        .await
        .unwrap());
    assert!(h.browser.last().is_none());
    assert_eq!(events.try_recv().unwrap().state, SignInState::NeedsSignIn);
}

// Scenario: Happy path — the person's identity for a handover
#[tokio::test]
async fn happy_path_the_person_s_identity_for_a_handover() {
    let h = Harness::new().await;
    h.seed(
        CredentialKind::Oidc,
        "asha.rao@acme.com",
        Seed {
            display_name: Some("Asha Rao"),
            ..Seed::default()
        },
    )
    .await;
    h.seed(CredentialKind::Github, "asharao", Seed::default())
        .await;
    let person = h.service.person().await.unwrap();
    assert_eq!(person.name, "Asha Rao");
    assert_eq!(person.email.as_deref(), Some("asha.rao@acme.com"));
    assert_eq!(person.github_login.as_deref(), Some("asharao"));
}

// Scenario: Happy path — an installation HMAC key is made once
#[tokio::test]
async fn happy_path_an_installation_hmac_key_is_made_once() {
    let h = Harness::new().await;
    let name = SecretName::new("sandbox/audit-hmac-key").unwrap();
    let store = h.service.secrets();
    let (a, b) = tokio::join!(
        store.get_or_create_random(&name, 32),
        store.get_or_create_random(&name, 32)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.len(), 32);
    assert_eq!(a.as_slice(), b.as_slice());
    assert_eq!(h.keychain.puts.load(Ordering::SeqCst), 1);
}

// Scenario: Happy path — the Remote API TLS key round-trips through ServerSecrets
#[tokio::test]
async fn happy_path_the_remote_api_tls_key_round_trips_through_serversecrets() {
    let h = Harness::with(Setup {
        program: Program::PersonalServer,
        store: super::harness::Store::File,
        browser: false,
        ..Setup::default()
    })
    .await;
    let pem = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIA==\n-----END PRIVATE KEY-----\n";
    h.service
        .put_remote_api_tls_key(zeroize::Zeroizing::new(pem.to_owned()))
        .await
        .unwrap();
    assert_eq!(
        h.service
            .remote_api_tls_key()
            .await
            .unwrap()
            .unwrap()
            .as_str(),
        pem
    );
    let name = SecretName::new("remote-api/tls-key").unwrap();
    assert!(h.service.secrets().get(&name).await.unwrap().is_some());
    h.service.delete_remote_api_tls_key().await.unwrap();
    h.service.delete_remote_api_tls_key().await.unwrap();
    assert!(h.service.remote_api_tls_key().await.unwrap().is_none());
}

// Scenario: Error — a stored desktop key of the wrong length
#[tokio::test]
async fn error_a_stored_desktop_key_of_the_wrong_length() {
    let h = Harness::new().await;
    let reference = "server-connector/desktop-key/0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f70";
    h.service
        .secrets()
        .put(&SecretName::new(reference).unwrap(), &[9u8; 16])
        .await
        .unwrap();
    assert_eq!(
        code_of(h.service.get_desktop_key(reference).await),
        "CRED_SECRET_STORE_CORRUPT"
    );
}

// Scenario: Error — the secrets key credential already exists
#[tokio::test]
async fn error_the_secrets_key_credential_already_exists() {
    let dir = super::harness::temp_dir();
    let path = dir.join("secrets.key.cred");
    std::fs::write(&path, b"credential").unwrap();
    let result = crate::IdentityService::create_secrets_key_credential(&path).await;
    assert_eq!(code_of(result), "CRED_SECRET_KEY_EXISTS");
    assert_eq!(std::fs::read(&path).unwrap(), b"credential");
}

// Scenario: Happy path — GitLab sign-in on the desktop by device flow (D9)
#[tokio::test]
async fn happy_path_gitlab_sign_in_on_the_desktop_by_device_flow_d9() {
    let h = Harness::new().await;
    Mock::given(method("POST"))
        .and(path("/gitlab.example.com/oauth/authorize_device"))
        .and(body_string_contains("scope=read_repository+write_repository+api"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "gl-dc", "user_code": "GLAB-1234",
            "verification_uri": "https://gitlab.example.com/oauth/device", "expires_in": 300, "interval": 1
        })))
        .expect(1)
        .mount(&h.mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/gitlab.example.com/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "glpat-oauth-1", "refresh_token": "gl-rt", "expires_in": 7200,
            "scope": "read_repository write_repository api"
        })))
        .mount(&h.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gitlab.example.com/api/v4/user"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"username": "asha.rao", "name": "Asha Rao"})),
        )
        .mount(&h.mock)
        .await;
    let gitlab = ProviderConfig::Gitlab {
        host: "gitlab.example.com".to_owned(),
        client_id: "a1b2c3".to_owned(),
    };
    let challenge = h
        .service
        .begin_sign_in(sign_in(gitlab, &[], SignInMethod::Browser))
        .await
        .unwrap();
    assert!(matches!(challenge, SignInChallenge::DeviceCode { .. }));
    assert_eq!(
        h.browser.last().as_deref(),
        Some("https://gitlab.example.com/oauth/device")
    );
    let credential = h
        .service
        .wait_for_sign_in(challenge_id(&challenge))
        .await
        .unwrap();
    assert_eq!(credential.kind, CredentialKind::Gitlab);
    assert_eq!(credential.account_label, "asha.rao");
}

async fn gitlab_token_mocks(h: &Harness, scopes: &[&str]) {
    Mock::given(method("GET"))
        .and(path("/gitlab.com/api/v4/personal_access_tokens/self"))
        .and(header("private-token", "glpat-pasted-0001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "active": true, "revoked": false, "scopes": scopes, "expires_at": "2027-01-31"
        })))
        .mount(&h.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gitlab.com/api/v4/user"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"username": "asharao", "name": "Asha Rao"})),
        )
        .mount(&h.mock)
        .await;
}

// Scenario: Happy path — a pasted GitLab token without an OAuth application
#[tokio::test]
async fn happy_path_a_pasted_gitlab_token_without_an_oauth_application() {
    let capture = Capture::default();
    let _guard = capture.install();
    let h = Harness::new().await;
    gitlab_token_mocks(&h, &["read_repository", "write_repository", "api"]).await;
    let mut events = h.service.subscribe();
    let credential = h
        .service
        .save_gitlab_token("gitlab.com", SecretString::new("glpat-pasted-0001".into()))
        .await
        .unwrap();
    assert_eq!(credential.kind, CredentialKind::Gitlab);
    let host: String = sqlx::query_scalar("SELECT provider_host FROM credential_meta")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(host, "gitlab.com");
    assert_eq!(events.try_recv().unwrap().state, SignInState::SignedIn);
    assert_eq!(
        credential.expires_at.unwrap().to_wire(),
        "2027-01-31T00:00:00.000Z"
    );
    assert_eq!(
        h.service
            .access_token(CredentialKind::Gitlab, None)
            .await
            .unwrap()
            .token
            .expose(),
        "glpat-pasted-0001"
    );
    let logs = capture.text();
    assert!(logs.contains("cred.sign_in.completed"));
    assert!(!logs.contains("glpat-pasted-0001"));
    assert!(!logs.contains("dropped_fields"));
}

// Scenario: Error path — a pasted GitLab token lacks a scope
#[tokio::test]
async fn error_path_a_pasted_gitlab_token_lacks_a_scope() {
    let h = Harness::new().await;
    gitlab_token_mocks(&h, &["read_repository"]).await;
    let result = h
        .service
        .save_gitlab_token("gitlab.com", SecretString::new("glpat-pasted-0001".into()))
        .await;
    assert_eq!(code_of(result), "CRED_SCOPE_MISSING");
    assert_eq!(h.count("SELECT count(*) FROM credentials").await, 0);
    assert!(h.keychain.items.lock().unwrap().is_empty());
}

const CERT: &[u8] = include_bytes!("../../tests/fixtures/acme-desktop.cert.pem");
const KEY: &[u8] = include_bytes!("../../tests/fixtures/acme-desktop.key.pem");
const OTHER_KEY: &[u8] = include_bytes!("../../tests/fixtures/other.key.pem");

// Scenario: Happy path — import a client certificate and read it back
#[tokio::test]
async fn happy_path_import_a_client_certificate_and_read_it_back() {
    let h = Harness::new().await;
    let view = h
        .service
        .import_client_certificate("ACME desktop", CERT, zeroize::Zeroizing::new(KEY.to_vec()))
        .await
        .unwrap();
    assert_eq!(view.subject, "ACME desktop");
    assert_eq!(view.not_after.to_wire(), "2099-06-30T00:00:00.000Z");
    let items = h
        .keychain
        .items
        .lock()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        items,
        vec![format!("client-certificate/{}", sha256_hex("ACME desktop"))]
    );
    let key = h
        .service
        .client_certificate("ACME desktop")
        .await
        .unwrap()
        .unwrap();
    let leaf = rustls::pki_types::CertificateDer::pem_slice_iter(CERT)
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(key.end_entity_cert().unwrap().as_ref(), leaf.as_ref());
    // Through the narrow interface too.
    assert!(
        DesktopKeyStore::client_certificate(&h.service, "ACME desktop")
            .await
            .unwrap()
            .is_some()
    );
}

fn sha256_hex(text: &str) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

use rustls::pki_types::pem::PemObject as _;

// Scenario: Error path — a key that does not match the certificate
#[tokio::test]
async fn error_path_a_key_that_does_not_match_the_certificate() {
    let h = Harness::new().await;
    let result = h
        .service
        .import_client_certificate(
            "ACME desktop",
            CERT,
            zeroize::Zeroizing::new(OTHER_KEY.to_vec()),
        )
        .await;
    assert_eq!(code_of(result), "CRED_CERTIFICATE_INVALID");
    assert!(h.keychain.items.lock().unwrap().is_empty());
}

// Scenario: Edge case — no certificate imported under the profile's name
#[tokio::test]
async fn edge_case_no_certificate_imported_under_the_profile_s_name() {
    let h = Harness::new().await;
    assert!(h
        .service
        .client_certificate("ACME desktop")
        .await
        .unwrap()
        .is_none());
}

// Scenario: Happy path — sign out
#[tokio::test]
async fn happy_path_sign_out() {
    let h = Harness::new().await;
    Mock::given(method("POST"))
        .and(path("/oauth2.googleapis.com/revoke"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&h.mock)
        .await;
    let id = h
        .seed(CredentialKind::Google, "asha@gmail.com", Seed::default())
        .await;
    let mut events = h.service.subscribe();
    h.service.sign_out(id).await.unwrap();
    assert!(h.keychain.items.lock().unwrap().is_empty());
    let state: String = sqlx::query_scalar("SELECT state FROM credentials")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(state, "revoked");
    assert_eq!(events.try_recv().unwrap().state, SignInState::SignedOut);
    // Idempotent on an already revoked row.
    h.service.sign_out(id).await.unwrap();
    h.mock.verify().await;
}

// ---- Amendment January 2026 (ADR-102)

fn slack(client_id: &str) -> ProviderConfig {
    ProviderConfig::Slack {
        workspace: "acme".to_owned(),
        client_id: client_id.to_owned(),
    }
}

// Scenario: a Slack sign-in that holds only the first scopes is asked for the later ones once
#[tokio::test]
async fn a_slack_sign_in_that_holds_only_the_first_scopes_is_asked_for_the_later_ones_once() {
    let _ports = PORTS.lock().await;
    let h = Harness::new().await;
    let id = h
        .seed(
            CredentialKind::Slack,
            "Acme/U0123",
            Seed {
                scopes: &SLACK_FIRST,
                ..Seed::default()
            },
        )
        .await;
    let later = ["users:read.email", "im:write"];
    assert_eq!(
        code_of(
            h.service
                .access_token_for(CredentialKind::Slack, &later)
                .await
        ),
        "CRED_SCOPE_MISSING"
    );
    let all = [SLACK_FIRST.join(","), later.join(",")].join(",");
    Mock::given(method("POST"))
        .and(path("/slack.com/api/oauth.v2.access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "team": {"name": "Acme"},
            "authed_user": {"id": "U0123", "access_token": "xoxp-2", "scope": all, "token_type": "user"}
        })))
        .mount(&h.mock)
        .await;
    let challenge = h
        .service
        .begin_sign_in(sign_in(
            slack("1234567890.9876543210"),
            &later,
            SignInMethod::Browser,
        ))
        .await
        .unwrap();
    let user_scope = query(&h.browser.last().unwrap())["user_scope"].clone();
    for scope in SLACK_FIRST.iter().chain(later.iter()) {
        assert!(user_scope.split(',').any(|s| s == *scope), "{scope}");
    }
    let credential = complete_browser(&h, &challenge).await;
    assert_eq!(credential.id, id);
    assert_eq!(h.count("SELECT count(*) FROM credentials").await, 1);
}

// Scenario: the person's own Slack app when the profile names none
#[tokio::test]
async fn the_person_s_own_slack_app_when_the_profile_names_none() {
    let _ports = PORTS.lock().await;
    let h = Harness::new().await;
    let challenge = h
        .service
        .begin_sign_in(sign_in(
            slack("1234567890.9876543210"),
            &[],
            SignInMethod::Browser,
        ))
        .await
        .unwrap();
    assert_eq!(
        query(&h.browser.last().unwrap())["client_id"],
        "1234567890.9876543210"
    );
    h.service.cancel_sign_in(challenge_id(&challenge)).unwrap();
}

// Scenario: no Slack client id at all
#[tokio::test]
async fn no_slack_client_id_at_all() {
    let h = Harness::new().await;
    let result = h
        .service
        .begin_sign_in(sign_in(slack(""), &[], SignInMethod::Browser))
        .await;
    assert_eq!(code_of(result), "CRED_PROVIDER_NOT_CONFIGURED");
    assert!(h.browser.last().is_none());
}

// Scenario: relayed Slack sign-in on the personal server
#[tokio::test]
async fn relayed_slack_sign_in_on_the_personal_server() {
    let _ports = PORTS.lock().await;
    let h = Harness::with(Setup {
        program: Program::PersonalServer,
        browser: false,
        ..Setup::default()
    })
    .await;
    Mock::given(method("POST"))
        .and(path("/slack.com/api/oauth.v2.access"))
        .and(body_string_contains("code=relayed-code"))
        .and(body_string_contains(
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A47821%2Fcallback",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "team": {"name": "Acme"},
            "authed_user": {"id": "U0123", "access_token": "xoxp-1", "scope": SLACK_FIRST.join(",")}
        })))
        .expect(1)
        .mount(&h.mock)
        .await;
    let mut request = sign_in(
        slack("1234567890.9876543210"),
        &[],
        SignInMethod::RelayedBrowser,
    );
    request.relay_port = Some(47821);
    let challenge = h.service.begin_sign_in(request).await.unwrap();
    let SignInChallenge::RelayedBrowser {
        challenge_id,
        authorisation_url,
        ..
    } = &challenge
    else {
        panic!("relayed challenge");
    };
    let q = query(authorisation_url);
    assert_eq!(q["redirect_uri"], "http://127.0.0.1:47821/callback");
    // No port is bound on the server.
    let probe = tokio::net::TcpListener::bind(("127.0.0.1", 47821)).await;
    assert!(probe.is_ok());
    drop(probe);
    let verifier_sent = q.contains_key("code_verifier");
    assert!(!verifier_sent, "the verifier never leaves the server");
    let waiter = {
        let id = *challenge_id;
        let service = crate::IdentityService {
            inner: std::sync::Arc::clone(&h.service.inner),
        };
        tokio::spawn(async move { service.wait_for_sign_in(id).await })
    };
    h.service
        .complete_relayed_sign_in(
            *challenge_id,
            SecretString::new("relayed-code".into()),
            q["state"].clone(),
        )
        .await
        .unwrap();
    let credential = waiter.await.unwrap().unwrap();
    assert_eq!(credential.kind, CredentialKind::Slack);
    assert_eq!(credential.account_label, "Acme/U0123");
    h.mock.verify().await;
}

// Scenario: relayed sign-in with the wrong state
#[tokio::test]
async fn relayed_sign_in_with_the_wrong_state() {
    let h = Harness::with(Setup {
        program: Program::PersonalServer,
        browser: false,
        ..Setup::default()
    })
    .await;
    let mut request = sign_in(
        slack("1234567890.9876543210"),
        &[],
        SignInMethod::RelayedBrowser,
    );
    request.relay_port = Some(47822);
    let challenge = h.service.begin_sign_in(request).await.unwrap();
    let id = challenge_id(&challenge);
    for _ in 0..3 {
        let result = h
            .service
            .complete_relayed_sign_in(id, SecretString::new("c".into()), "wrong".into())
            .await;
        assert_eq!(code_of(result), "CRED_RELAY_STATE_MISMATCH");
    }
    // Three mismatches end the challenge.
    assert_eq!(
        code_of(h.service.wait_for_sign_in(id).await),
        "CRED_RELAY_STATE_MISMATCH"
    );
    assert!(
        h.mock.received_requests().await.unwrap().is_empty(),
        "no token request"
    );
}

// Scenario: relayed sign-in refused on the desktop
#[tokio::test]
async fn relayed_sign_in_refused_on_the_desktop() {
    let h = Harness::new().await;
    let mut request = sign_in(
        slack("1234567890.9876543210"),
        &[],
        SignInMethod::RelayedBrowser,
    );
    request.relay_port = Some(47821);
    assert_eq!(
        code_of(h.service.begin_sign_in(request).await),
        "CRED_FLOW_NOT_OFFERED"
    );
}

// ---- Amendment January 2026 (ADR-106)

async fn save_acme(
    h: &Harness,
    token: &str,
) -> Result<ordin_types::domain::identity::Credential, crate::CredError> {
    h.service
        .save_jira_token(
            "acme.atlassian.net",
            "sam.lee@acme.co.uk",
            SecretString::new(token.to_owned()),
            "5b10ac8d82e05b22cc7d4ef5",
            "Sam Lee",
        )
        .await
}

// Scenario: Happy path — a pasted Jira token with no company set-up
#[tokio::test]
async fn happy_path_a_pasted_jira_token_with_no_company_set_up() {
    let capture = Capture::default();
    let _guard = capture.install();
    let h = Harness::new().await;
    let mut events = h.service.subscribe();
    let credential = save_acme(&h, "ATATT-jira-token-0001").await.unwrap();
    assert_eq!(credential.kind, CredentialKind::Jira);
    assert_eq!(credential.account_label, "acme.atlassian.net");
    assert_eq!(events.try_recv().unwrap().state, SignInState::SignedIn);
    assert_eq!(
        credential.keychain_ref,
        format!("credential/jira/{}", credential.id)
    );
    let blob = h.blob(CredentialKind::Jira, credential.id).await.unwrap();
    assert_eq!(blob["access_token"], "ATATT-jira-token-0001");
    assert_eq!(blob["method"], "api_token");
    assert_eq!(blob["refresh_token"], serde_json::Value::Null);
    assert!(!h.table_text().await.contains("ATATT-jira-token-0001"));
    let view = serde_json::to_string(&h.service.list().await.unwrap()).unwrap();
    assert!(!view.contains("ATATT-jira-token-0001"));
    assert!(!capture.text().contains("ATATT-jira-token-0001"));
    let access = h.service.jira_access().await.unwrap().unwrap();
    assert_eq!(access.site, "acme.atlassian.net");
    assert_eq!(
        access.method,
        JiraCredentialMethod::ApiToken {
            email: "sam.lee@acme.co.uk".to_owned()
        }
    );
    assert_eq!(access.token.token.expose(), "ATATT-jira-token-0001");
}

// Scenario: Happy path — Jira through the company's Atlassian app
#[tokio::test]
async fn happy_path_jira_through_the_company_s_atlassian_app() {
    let _ports = PORTS.lock().await;
    let h = Harness::new().await;
    Mock::given(method("POST"))
        .and(path("/auth.atlassian.com/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "atl-access", "refresh_token": "atl-refresh", "expires_in": 3600,
            "scope": "read:jira-work write:jira-work read:jira-user offline_access"
        })))
        .mount(&h.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api.atlassian.com/oauth/token/accessible-resources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "1", "url": "https://acme.atlassian.net", "scopes": ["read:jira-work", "write:jira-work"]}
        ])))
        .mount(&h.mock)
        .await;
    let jira = ProviderConfig::Jira {
        site: "https://acme.atlassian.net".to_owned(),
        client_id: "atl-123".to_owned(),
    };
    let challenge = h
        .service
        .begin_sign_in(sign_in(jira, &[], SignInMethod::Browser))
        .await
        .unwrap();
    let url = h.browser.last().unwrap();
    assert!(url.starts_with("https://auth.atlassian.com/authorize?"));
    assert_eq!(
        query(&url)["scope"],
        "read:jira-work write:jira-work read:jira-user offline_access"
    );
    let credential = complete_browser(&h, &challenge).await;
    assert_eq!(credential.account_label, "acme.atlassian.net");
    let access = h.service.jira_access().await.unwrap().unwrap();
    assert_eq!(access.method, JiraCredentialMethod::Oauth);
    assert_eq!(access.token.token.expose(), "atl-access");
}

// Scenario: Edge case — no Jira credential
#[tokio::test]
async fn edge_case_no_jira_credential() {
    let h = Harness::new().await;
    assert!(h.service.jira_access().await.unwrap().is_none());
}

// Scenario: Error path — a second Jira site
#[tokio::test]
async fn error_path_a_second_jira_site() {
    let h = Harness::new().await;
    let first = save_acme(&h, "token-acme").await.unwrap();
    let result = h
        .service
        .save_jira_token(
            "other.atlassian.net",
            "sam.lee@acme.co.uk",
            SecretString::new("t2".into()),
            "5b10",
            "Sam Lee",
        )
        .await;
    assert_eq!(code_of(result), "CRED_DUPLICATE");
    let access = h.service.jira_access().await.unwrap().unwrap();
    assert_eq!(access.site, "acme.atlassian.net");
    assert_eq!(access.token.token.expose(), "token-acme");
    assert_eq!(access.token.credential_id, first.id);
}

// Scenario: Error path — Jira on the server
#[tokio::test]
async fn error_path_jira_on_the_server() {
    let h = Harness::with(Setup {
        program: Program::PersonalServer,
        browser: false,
        ..Setup::default()
    })
    .await;
    let jira = ProviderConfig::Jira {
        site: "https://acme.atlassian.net".to_owned(),
        client_id: "atl-123".to_owned(),
    };
    for method in [SignInMethod::Browser, SignInMethod::DeviceCode] {
        assert_eq!(
            code_of(
                h.service
                    .begin_sign_in(sign_in(jira.clone(), &[], method))
                    .await
            ),
            "CRED_FLOW_NOT_OFFERED"
        );
    }
    assert_eq!(code_of(save_acme(&h, "t").await), "CRED_FLOW_NOT_OFFERED");
}

// Scenario: Edge case — the migration keeps every credential
// (the migration test proper, on steps 1 and 2 applied one after the other, is
// `tests/migrations.rs`; this one checks the same on the harness's migrated file.)
#[tokio::test]
async fn edge_case_the_migration_keeps_every_credential() {
    let h = Harness::new().await;
    h.seed(CredentialKind::Github, "asharao", Seed::default())
        .await;
    h.seed(
        CredentialKind::Confluence,
        "acme.atlassian.net",
        Seed::default(),
    )
    .await;
    save_acme(&h, "ATATT-jira-token-0002").await.unwrap();
    assert_eq!(h.count("SELECT count(*) FROM credentials").await, 3);
    assert_eq!(h.count("SELECT count(*) FROM credential_meta").await, 3);
    let problems = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&h.pool)
        .await
        .unwrap();
    assert!(problems.is_empty());
}
