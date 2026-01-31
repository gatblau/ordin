//! X1: one test per row of the Error Table, named after its code.

use std::sync::Arc;

use ordin_types::KnowledgeConnectionId;
use serde_json::json;
use wiremock::ResponseTemplate;

use super::harness::*;
use crate::*;

async fn add_err(fx: &Fixture, url: &str) -> KqError {
    fx.client
        .add_connection(fx.ws, url)
        .await
        .expect_err("the command fails")
}

async fn row_count(fx: &Fixture) -> i64 {
    use sqlx::Row;
    sqlx::query("SELECT COUNT(*) AS n FROM kq_connections")
        .fetch_one(&fx.pool())
        .await
        .unwrap()
        .get("n")
}

#[tokio::test]
async fn kq_url_invalid() {
    let fx = Fixture::new().await;
    let long = format!("https://{}.example.org", "a".repeat(2_100));
    for url in [
        "http://kfd.example.org",
        "ftp://kfd.example.org",
        "not a url",
        "https://user:pw@kfd.example.org",
        "https://user@kfd.example.org",
        "https://kfd.example.org/#frag",
        "https://",
        "",
        long.as_str(),
    ] {
        assert_eq!(add_err(&fx, url).await, KqError::UrlInvalid, "{url}");
    }
    assert_eq!(row_count(&fx).await, 0);
}

#[tokio::test]
async fn kq_too_many_connections() {
    let fx = Fixture::new().await;
    for n in 0..8 {
        let host = format!("kfd{n}.example.org");
        let _agent = fx.agent(&host, &format!("agent-{n}")).await;
        fx.client
            .add_connection(fx.ws, &format!("https://{host}"))
            .await
            .expect("within the limit");
    }
    let _ninth = fx.agent("kfd8.example.org", "agent-8").await;
    assert_eq!(
        add_err(&fx, "https://kfd8.example.org").await,
        KqError::TooManyConnections
    );
    // Another workspace is not affected.
    let other = ordin_types::WorkspaceId::new();
    fx.client
        .add_connection(other, "https://kfd8.example.org")
        .await
        .expect("another workspace has its own limit");
}

#[tokio::test]
async fn kq_already_connected() {
    let fx = Fixture::new().await;
    let _agent = fx.agent("kfd.example.org", "procurement").await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    // The host is lower-cased and a trailing slash dropped before the comparison.
    assert_eq!(
        add_err(&fx, "https://KFD.example.org/").await,
        KqError::AlreadyConnected
    );
}

#[tokio::test]
async fn kq_not_a_knowledge_agent() {
    let fx = Fixture::new().await;
    let not_found = fx.stand_in("a.example.org", "a").await;
    not_found
        .route("GET", "/v1/knowledge/about", ResponseTemplate::new(404))
        .await;
    let not_json = fx.stand_in("b.example.org", "b").await;
    not_json
        .route(
            "GET",
            "/v1/knowledge/about",
            ResponseTemplate::new(200).set_body_string("<html>"),
        )
        .await;
    let wrong_shape = fx.stand_in("c.example.org", "c").await;
    wrong_shape.about(json!({ "agent": "c" })).await;
    let empty_name = fx.stand_in("d.example.org", "d").await;
    empty_name.about(about_json("", ISSUER, "aud")).await;
    for host in ["a", "b", "c", "d"] {
        assert_eq!(
            add_err(&fx, &format!("https://{host}.example.org")).await,
            KqError::NotAKnowledgeAgent,
            "{host}"
        );
    }
    assert_eq!(row_count(&fx).await, 0);
}

#[tokio::test]
async fn kq_other_company() {
    let fx = Fixture::new().await;
    let agent = fx.stand_in("kfd.example.org", "other").await;
    agent
        .about(about_json(
            "other",
            "https://id.other-company.example",
            "aud",
        ))
        .await;
    assert_eq!(
        add_err(&fx, "https://kfd.example.org").await,
        KqError::OtherCompany
    );
    assert_eq!(row_count(&fx).await, 0);
}

#[tokio::test]
async fn kq_sign_in_needed() {
    let fx = Fixture::new().await;
    let _agent = fx.agent("kfd.example.org", "procurement").await;
    *fx.tokens.audience_mode.lock().unwrap() = AudienceMode::Permanent;
    assert_eq!(
        add_err(&fx, "https://kfd.example.org").await,
        KqError::SignInNeeded
    );

    // A token refused twice is the same answer: the contents route says 401 both times.
    let fx = Fixture::new().await;
    let agent = fx.stand_in("kfd.example.org", "procurement").await;
    agent
        .about(about_json("procurement", ISSUER, "aud-procurement"))
        .await;
    agent
        .route("GET", "/v1/knowledge/contents", ResponseTemplate::new(401))
        .await;
    assert_eq!(
        add_err(&fx, "https://kfd.example.org").await,
        KqError::SignInNeeded
    );
    assert_eq!(
        fx.tokens
            .invalidated
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(row_count(&fx).await, 0);
}

#[tokio::test]
async fn kq_not_in_audience() {
    let fx = Fixture::new().await;
    let agent = fx.stand_in("kfd.example.org", "procurement").await;
    agent
        .about(about_json("procurement", ISSUER, "aud-procurement"))
        .await;
    agent
        .route(
            "GET",
            "/v1/knowledge/contents",
            ResponseTemplate::new(403).set_body_json(json!({
                "error": "This knowledge agent serves another group of people.",
                "code": "KQ_NOT_IN_AUDIENCE"
            })),
        )
        .await;
    assert_eq!(
        add_err(&fx, "https://kfd.example.org").await,
        KqError::NotInAudience
    );
    assert_eq!(row_count(&fx).await, 0);
}

#[tokio::test]
async fn kq_unreachable() {
    let fx = Fixture::new().await;
    // A closed loopback port: the connection is refused.
    fx.client.inner.transport.rewrites.lock().unwrap().insert(
        "https://down.example.org".to_owned(),
        "http://127.0.0.1:1".to_owned(),
    );
    assert_eq!(
        add_err(&fx, "https://down.example.org").await,
        KqError::Unreachable
    );

    // The contents route fails with a server error.
    let agent = fx.stand_in("kfd.example.org", "procurement").await;
    agent
        .about(about_json("procurement", ISSUER, "aud-procurement"))
        .await;
    agent
        .route("GET", "/v1/knowledge/contents", ResponseTemplate::new(500))
        .await;
    assert_eq!(
        add_err(&fx, "https://kfd.example.org").await,
        KqError::Unreachable
    );

    // A 403 that is not KQ_NOT_IN_AUDIENCE is "any other failure".
    let other = fx.stand_in("kfd2.example.org", "procurement").await;
    other
        .about(about_json("procurement", ISSUER, "aud-procurement"))
        .await;
    other
        .route("GET", "/v1/knowledge/contents", ResponseTemplate::new(403))
        .await;
    assert_eq!(
        add_err(&fx, "https://kfd2.example.org").await,
        KqError::Unreachable
    );

    // The sign-in service failed temporarily.
    let agent = fx.agent("kfd3.example.org", "third").await;
    let _ = &agent;
    *fx.tokens.audience_mode.lock().unwrap() = AudienceMode::Retryable;
    assert_eq!(
        add_err(&fx, "https://kfd3.example.org").await,
        KqError::Unreachable
    );
    assert_eq!(row_count(&fx).await, 0);
}

#[tokio::test]
async fn kq_connection_not_found() {
    let fx = Fixture::new().await;
    let unknown = KnowledgeConnectionId::new();
    assert_eq!(
        fx.client.remove_connection(unknown).await.unwrap_err(),
        KqError::ConnectionNotFound
    );
    assert_eq!(
        fx.client.details(unknown).await.unwrap_err(),
        KqError::ConnectionNotFound
    );
    // A company front door the person already removed is no longer there for the person.
    let _agent = fx.agent("kfd.example.org", "procurement").await;
    fx.client
        .apply_company_front_doors(vec![(
            fx.ws,
            vec![company("hq", "https://kfd.example.org")],
        )])
        .await
        .unwrap();
    let id = fx.client.list_connections(fx.ws).await.unwrap()[0].connection_id;
    fx.client.remove_connection(id).await.unwrap();
    assert_eq!(
        fx.client.remove_connection(id).await.unwrap_err(),
        KqError::ConnectionNotFound
    );
    assert_eq!(
        fx.client.details(id).await.unwrap_err(),
        KqError::ConnectionNotFound
    );
}

pub(crate) fn company(key: &str, url: &str) -> CompanyFrontDoor {
    CompanyFrontDoor {
        key: key.to_owned(),
        display_name: format!("Company {key}"),
        url: url.to_owned(),
        applies_to: AppliesTo::TeamKnowledge,
    }
}

#[tokio::test]
async fn kq_not_supported() {
    let fx = Fixture::agent_mode(&["p1"]).await;
    let id = KnowledgeConnectionId::new();
    assert_eq!(
        add_err(&fx, "https://kfd.example.org").await,
        KqError::NotSupported
    );
    assert_eq!(
        fx.client.list_connections(fx.ws).await.unwrap_err(),
        KqError::NotSupported
    );
    assert_eq!(
        fx.client.remove_connection(id).await.unwrap_err(),
        KqError::NotSupported
    );
    assert_eq!(
        fx.client.details(id).await.unwrap_err(),
        KqError::NotSupported
    );
    assert_eq!(
        fx.client
            .apply_company_front_doors(vec![])
            .await
            .unwrap_err(),
        KqError::NotSupported
    );
    // Proxies asked in Person mode.
    let person = Fixture::new().await;
    let ask = person.ask(
        AskTarget::Proxies(vec![ProxyAsk {
            primary: "p1".into(),
            backup: None,
        }]),
        "question",
    );
    assert!(matches!(
        person.client.ask(ask).await,
        Err(KqError::NotSupported)
    ));
}

#[tokio::test]
async fn kq_config_invalid() {
    let dir = TempDir::new();
    let journal = open_journal(&dir).await;
    // Person mode with no Identity.
    let person = KnowledgeQueryClient::new(KnowledgeClientDeps {
        journal: Arc::clone(&journal),
        identity: None,
        http: reqwest::Client::new(),
        mode: KnowledgeClientMode::Person {
            sign_in: CompanySignIn {
                issuer: ISSUER.into(),
                client_id: CLIENT_ID.into(),
            },
        },
    });
    assert!(matches!(person, Err(KqError::ConfigInvalid)));
    // Proxies with no service token.
    let agent = KnowledgeQueryClient::new(KnowledgeClientDeps {
        journal: Arc::clone(&journal),
        identity: None,
        http: reqwest::Client::new(),
        mode: KnowledgeClientMode::Agent {
            proxies: vec![proxy("p1")],
            service_token: None,
        },
    });
    assert!(matches!(agent, Err(KqError::ConfigInvalid)));
    // No proxies and no service token is allowed (AgentConfiguration SA-ACFG-KA-02).
    let none = KnowledgeQueryClient::new(KnowledgeClientDeps {
        journal,
        identity: None,
        http: reqwest::Client::new(),
        mode: KnowledgeClientMode::Agent {
            proxies: vec![],
            service_token: None,
        },
    });
    assert!(none.is_ok());
}

#[tokio::test]
async fn kq_bad_query() {
    let fx = Fixture::new().await;
    let search = |q: &str| ordin_types::wire::knowledge_query::KnowledgeSearch {
        kind: ordin_types::wire::knowledge_query::KnowledgeSearchKind::Lex,
        query: q.to_owned(),
    };
    type Edit = Box<dyn Fn(&mut KnowledgeAsk)>;
    let cases: Vec<(&str, Edit)> = vec![
        ("neither query nor searches", Box::new(|a| a.query = None)),
        (
            "both query and searches",
            Box::new(move |a| a.searches = Some(vec![search("x")])),
        ),
        ("empty query", Box::new(|a| a.query = Some(String::new()))),
        (
            "long query",
            Box::new(|a| a.query = Some("q".repeat(4_097))),
        ),
        (
            "no searches",
            Box::new(|a| {
                a.query = None;
                a.searches = Some(vec![]);
            }),
        ),
        (
            "five searches",
            Box::new(move |a| {
                a.query = None;
                a.searches = Some((0..5).map(|_| search("x")).collect());
            }),
        ),
        (
            "empty search",
            Box::new(move |a| {
                a.query = None;
                a.searches = Some(vec![search("")]);
            }),
        ),
        (
            "long search",
            Box::new(move |a| {
                a.query = None;
                a.searches = Some(vec![search(&"s".repeat(501))]);
            }),
        ),
        ("limit zero", Box::new(|a| a.limit = 0)),
        ("limit 51", Box::new(|a| a.limit = 51)),
        ("hop budget 3", Box::new(|a| a.hop_budget = 3)),
        (
            "nine visited",
            Box::new(|a| a.visited = (0..9).map(|n| n.to_string()).collect()),
        ),
        (
            "empty visited name",
            Box::new(|a| a.visited = vec![String::new()]),
        ),
        (
            "long visited name",
            Box::new(|a| a.visited = vec!["v".repeat(81)]),
        ),
    ];
    for (name, edit) in cases {
        let mut ask = fx.ask(AskTarget::Workspace(fx.ws), "question");
        edit(&mut ask);
        assert!(
            matches!(fx.client.ask(ask).await, Err(KqError::BadQuery)),
            "{name}"
        );
    }
}

#[tokio::test]
async fn kq_no_connections() {
    let fx = Fixture::new().await;
    let ask = fx.ask(AskTarget::Workspace(fx.ws), "question");
    assert!(matches!(
        fx.client.ask(ask).await,
        Err(KqError::NoConnections)
    ));
    // The agent profile has no workspaces.
    let agent = Fixture::agent_mode(&["p1"]).await;
    let ask = agent.ask(AskTarget::Workspace(agent.ws), "question");
    assert!(matches!(
        agent.client.ask(ask).await,
        Err(KqError::NoConnections)
    ));
    // A connection that needs a sign-in, or that is outside the audience, is not asked.
    let _agent = fx.agent("kfd.example.org", "procurement").await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    sqlx::query("UPDATE kq_connections SET state = 'not_in_audience'")
        .execute(&fx.pool())
        .await
        .unwrap();
    let ask = fx.ask(AskTarget::Workspace(fx.ws), "question");
    assert!(matches!(
        fx.client.ask(ask).await,
        Err(KqError::NoConnections)
    ));
}

#[tokio::test]
async fn kq_unknown_proxy() {
    let fx = Fixture::agent_mode(&["p1"]).await;
    for target in [
        vec![ProxyAsk {
            primary: "nobody".into(),
            backup: None,
        }],
        vec![ProxyAsk {
            primary: "p1".into(),
            backup: Some("nobody".into()),
        }],
    ] {
        let ask = fx.ask(AskTarget::Proxies(target), "question");
        assert!(matches!(
            fx.client.ask(ask).await,
            Err(KqError::UnknownProxy)
        ));
    }
}

#[tokio::test]
async fn kq_store_unavailable() {
    let fx = Fixture::new().await;
    let pool = fx.pool();
    for statement in [
        "DROP TABLE kq_connection_accounts",
        "DROP TABLE kq_connection_contents",
        "DROP TABLE kq_connections",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    assert_eq!(
        fx.client.list_connections(fx.ws).await.unwrap_err(),
        KqError::StoreUnavailable
    );
    let _agent = fx.agent("kfd.example.org", "procurement").await;
    assert_eq!(
        add_err(&fx, "https://kfd.example.org").await,
        KqError::StoreUnavailable
    );
    // The Memory Service's reads never fail: a store error gives an empty answer.
    assert!(fx.client.connections_of(fx.ws).await.is_empty());
    assert!(fx.client.contents_names(fx.ws).await.is_empty());
    assert!(fx.client.microsoft_check_scopes().await.is_empty());
}
