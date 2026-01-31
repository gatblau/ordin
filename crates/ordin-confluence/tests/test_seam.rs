//! CONF-SEAM-2: a test's gateway, site and token in place of Atlassian's gateway, the profile's
//! site and Identity's token, on an adapter with no site in its settings and nobody signed in.

mod support;

use ordin_confluence::{ConfluenceLocation, ConfluenceTestSeam};
use serde_json::json;
use support::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, ResponseTemplate};

const PAGE: &str = "4411";
const SEAM_TOKEN: &str = "seam-token-0001";

fn location() -> ConfluenceLocation {
    ConfluenceLocation {
        space_key: "PROC".to_owned(),
        space_name: "Procurement".to_owned(),
        page_id: Some(PAGE.to_owned()),
        title: "Rates".to_owned(),
        web_url: String::new(),
    }
}

fn restrictions() -> String {
    api(&format!(
        "/rest/api/content/{PAGE}/restriction/byOperation/read"
    ))
}

async fn unseamed() -> Env {
    Env::with(Options {
        sign_in: SignIn::Nobody,
        site: None,
        ..Options::default()
    })
    .await
}

fn seam(env: &Env) -> ConfluenceTestSeam {
    ConfluenceTestSeam {
        gateway: env.mock.uri(),
        site: SITE.parse().expect("the site"),
        token: SEAM_TOKEN.to_owned(),
    }
}

// spec: ordin-desktop/components/ConfluenceAdapter#CONF-SEAM-2
#[tokio::test]
async fn the_seam_gives_the_gateway_site_and_token() {
    let env = unseamed().await;
    let refused = env.adapter.audience(&location()).await.unwrap_err();
    assert_eq!(
        refused.code(),
        "CONF_SIGN_IN_NEEDED",
        "nobody signed in, no seam"
    );

    env.adapter
        .use_test_seam(seam(&env))
        .expect("a loopback seam");
    Mock::given(method("GET"))
        .and(path(restrictions()))
        .and(header(
            "authorization",
            format!("Bearer {SEAM_TOKEN}").as_str(),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"restrictions": {
                "user": {"results": []},
                "group": {"results": [{"name": "Finance", "id": "g-1"}]},
            }})),
        )
        .expect(1)
        .mount(&env.mock)
        .await;
    let audience = env
        .adapter
        .audience(&location())
        .await
        .expect("the audience");
    assert_eq!(audience.line, "Finance");
}

// spec: ordin-desktop/components/ConfluenceAdapter#CONF-SEAM-2
#[tokio::test]
async fn error_path_a_seam_off_loopback_over_http_is_refused() {
    let env = unseamed().await;
    let refused = env
        .adapter
        .use_test_seam(ConfluenceTestSeam {
            gateway: "http://gateway.example".to_owned(),
            ..seam(&env)
        })
        .unwrap_err();
    assert_eq!(refused.code(), "CONF_NOT_CONFIGURED");
    let still = env.adapter.audience(&location()).await.unwrap_err();
    assert_eq!(still.code(), "CONF_SIGN_IN_NEEDED", "nothing changed");
    assert!(env.requests().await.is_empty());
}

// spec: ordin-desktop/components/ConfluenceAdapter#CONF-SEAM-2
#[tokio::test]
async fn error_path_a_refused_seam_token_is_sign_in_needed_at_once() {
    let env = unseamed().await;
    env.adapter
        .use_test_seam(seam(&env))
        .expect("a loopback seam");
    status_only(&env, "GET", &restrictions(), 401).await;
    let refused = env.adapter.audience(&location()).await.unwrap_err();
    assert_eq!(refused.code(), "CONF_SIGN_IN_NEEDED");
    assert_eq!(
        env.count("GET", &restrictions()).await,
        1,
        "no refresh, no second attempt"
    );
}
