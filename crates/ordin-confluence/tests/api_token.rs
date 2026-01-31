//! Confluence connected with a pasted API token (ConfluenceAdapter.md CONF-TOKEN-1, CONF-TOKEN-2; amended
//! January 2026; user decision; Approved by the user January 2026): the credential check against a
//! `wiremock` Atlassian site (success, 401, a wrong site), every later request sent to the site with Basic
//! authentication, and the token never in a log line or a reply.
//!
//! The log subscriber is installed per test on the test's own thread with `set_default`.

mod support;

use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ordin_identity::SecretString;
use serde_json::{json, Value};
use support::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, ResponseTemplate};

const HOST: &str = "acme.atlassian.net";
const EMAIL: &str = "sam@acme.com";
const API_TOKEN: &str = "ATATT3xFfGF0-confluence-api-token-0001";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let writer = self.clone();
        let subscriber = tracing_subscriber::fmt()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }
}

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn basic() -> String {
    format!("Basic {}", STANDARD.encode(format!("{EMAIL}:{API_TOKEN}")))
}

/// Nobody signed in and no site in the profile: the default organisation profile names no Confluence app.
async fn env() -> Env {
    Env::with(Options {
        sign_in: SignIn::Nobody,
        site: None,
        ..Options::default()
    })
    .await
}

async fn mount_current_user(env: &Env, status: u16, body: Value) {
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/user/current"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&env.mock)
        .await;
}

fn sam() -> Value {
    json!({
        "type": "known",
        "accountId": "5b10a2844c20165700ede21g",
        "accountType": "atlassian",
        "displayName": "Sam Lee",
        "email": EMAIL
    })
}

async fn connect(env: &Env, site: &str) -> Result<ordin_confluence::ConfluenceUser, String> {
    env.adapter
        .connect_with_token(site, EMAIL, SecretString::new(API_TOKEN.to_owned()))
        .await
        .map_err(|e| e.code().to_owned())
}

async fn kept(env: &Env) -> bool {
    env.identity
        .confluence_token_access()
        .await
        .expect("identity answers")
        .is_some()
}

// spec: ordin-desktop/components/ConfluenceAdapter#CONF-TOKEN-1
#[tokio::test(flavor = "current_thread")]
async fn happy_path_a_checked_token_is_kept_and_signs_every_later_request() {
    let env = env().await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/user/current"))
        .and(header("authorization", basic().as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(sam()))
        .mount(&env.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .and(header("authorization", basic().as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .mount(&env.mock)
        .await;
    let capture = Capture::default();
    let guard = capture.install();

    let person = connect(&env, HOST).await.expect("accepted");
    let found = env.adapter.find_locations("rates", 5).await;

    drop(guard);
    assert_eq!(person.display_name, "Sam Lee");
    assert_eq!(person.account_id, "5b10a2844c20165700ede21g");
    assert!(found.expect("a read with the API token").is_empty());
    let access = env
        .identity
        .confluence_token_access()
        .await
        .unwrap()
        .expect("kept");
    assert_eq!(access.site, HOST);
    assert_eq!(access.email, EMAIL);
    // CONF-TOKEN-2: straight to the site, never through the gateway's site look-up.
    assert_eq!(
        env.requests().await,
        vec![
            ("GET".to_owned(), "/wiki/rest/api/user/current".to_owned()),
            ("GET".to_owned(), "/wiki/rest/api/search".to_owned()),
        ]
    );
    // The token is in no log line and no reply.
    let text = capture.text();
    assert!(text.contains("confluence_token_checked"), "{text}");
    assert!(!text.contains(API_TOKEN), "{text}");
    assert!(!text.contains(&basic()), "{text}");
    assert!(!format!("{person:?}").contains(API_TOKEN));
    assert!(!serde_json::to_string(&person).unwrap().contains(API_TOKEN));
}

#[tokio::test]
async fn error_path_a_refused_token_is_token_refused_and_nothing_is_kept() {
    let env = env().await;
    mount_current_user(&env, 401, json!({"message": "Unauthorized"})).await;

    assert_eq!(connect(&env, HOST).await.unwrap_err(), "CONF_TOKEN_REFUSED");
    assert!(!kept(&env).await);
}

#[tokio::test]
async fn error_path_the_anonymous_user_is_a_refused_token() {
    let env = env().await;
    mount_current_user(
        &env,
        200,
        json!({"type": "anonymous", "displayName": "Anonymous"}),
    )
    .await;

    assert_eq!(connect(&env, HOST).await.unwrap_err(), "CONF_TOKEN_REFUSED");
    assert!(!kept(&env).await);
}

#[tokio::test]
async fn error_path_a_site_with_no_confluence_is_site_invalid() {
    let env = env().await;
    mount_current_user(&env, 404, json!({"message": "Not Found"})).await;

    assert_eq!(connect(&env, HOST).await.unwrap_err(), "CONF_SITE_INVALID");
    assert!(!kept(&env).await);
}

#[tokio::test]
async fn error_path_a_reply_that_is_not_a_confluence_user_is_site_invalid() {
    let env = env().await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/user/current"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>Sign in</html>"))
        .mount(&env.mock)
        .await;

    assert_eq!(connect(&env, HOST).await.unwrap_err(), "CONF_SITE_INVALID");
    assert!(!kept(&env).await);
}

#[tokio::test]
async fn error_path_an_address_that_is_not_an_atlassian_site_is_never_called() {
    let env = env().await;

    for bad in [
        "confluence.example.com",
        "https://acme.atlassian.net/wiki",
        "not a host",
    ] {
        assert_eq!(
            connect(&env, bad).await.unwrap_err(),
            "CONF_SITE_INVALID",
            "{bad}"
        );
    }
    let refused = env
        .adapter
        .connect_with_token(HOST, "sam", SecretString::new(API_TOKEN.to_owned()))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), "CONF_TOKEN_REFUSED");
    assert!(env.requests().await.is_empty());
}

#[tokio::test]
async fn error_path_an_atlassian_outage_is_unavailable_and_retryable() {
    let env = env().await;
    mount_current_user(&env, 503, json!({})).await;

    let error = env
        .adapter
        .connect_with_token(HOST, EMAIL, SecretString::new(API_TOKEN.to_owned()))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_UNAVAILABLE");
    assert!(error.retryable());
    assert!(!kept(&env).await);
}

// spec: ordin-desktop/components/ConfluenceAdapter#CONF-TOKEN-2
#[tokio::test]
async fn error_path_a_kept_token_later_refused_asks_for_a_new_one() {
    let env = env().await;
    mount_current_user(&env, 200, sam()).await;
    connect(&env, HOST).await.expect("accepted");
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&env.mock)
        .await;

    let error = env.adapter.find_locations("rates", 5).await.unwrap_err();

    assert_eq!(error.code(), "CONF_SIGN_IN_NEEDED");
    let error = env.adapter.find_locations("rates", 5).await.unwrap_err();
    assert_eq!(error.code(), "CONF_SIGN_IN_NEEDED");
}
