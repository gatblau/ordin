//! Behaviour the Gherkin scenarios do not reach: the per-agent failure rules of `ask`, the service
//! token, the contents cache, linking edge cases, company reconciliation and the change streams.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_types::domain::identity::CredentialKind;
use ordin_types::wire::knowledge_query::{KnowledgeContents, RepositoryMoved, UnansweredReason};
use ordin_types::KnowledgeAnswerId;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::error_table::company;
use super::harness::*;
use crate::client::Timings;
use crate::*;

/// A person with one connected front door, background tasks off.
async fn connected(host: &str, name: &str) -> (Fixture, Agent) {
    let fx = Fixture::new().await;
    let agent = fx.agent(host, name).await;
    fx.client
        .add_connection(fx.ws, &format!("https://{host}"))
        .await
        .expect("connects");
    (fx, agent)
}

async fn first_batch(fx: &Fixture) -> KnowledgeBatch {
    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "question"))
        .await
        .unwrap();
    stream.first().await
}

fn reason_of(batch: &KnowledgeBatch, agent: &str) -> Option<&'static str> {
    batch
        .unanswered
        .iter()
        .find(|u| u.agent == agent)
        .map(|u| match u.reason {
            UnansweredReason::NotInTime => "not_in_time",
            UnansweredReason::Refused => "refused",
            UnansweredReason::Unreachable => "unreachable",
        })
}

async fn state_of(fx: &Fixture) -> String {
    use sqlx::Row;
    sqlx::query("SELECT state FROM kq_connections")
        .fetch_one(&fx.pool())
        .await
        .unwrap()
        .get("state")
}

// ---- ask: the rules for one agent's failure

#[tokio::test]
async fn a_401_invalidates_the_token_and_retries_once_within_the_deadline() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    Mock::given(method("POST"))
        .and(path("/v1/knowledge/query"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&agent.mock)
        .await;
    agent.answers(&[hit("kfd", "a.md", "a fact")]).await;
    let batch = first_batch(&fx).await;
    assert_eq!(batch.hits.len(), 1);
    assert_eq!(fx.tokens.invalidated.load(Ordering::SeqCst), 1);
    let requests = agent.query_requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        header_of(&requests[1], "authorization"),
        format!("Bearer {}", fx.tokens.bearer("aud-kfd"))
    );
    assert_ne!(
        header_of(&requests[0], "authorization"),
        header_of(&requests[1], "authorization")
    );
}

#[tokio::test]
async fn a_second_401_is_refused() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent.query(ResponseTemplate::new(401)).await;
    let batch = first_batch(&fx).await;
    assert!(batch.hits.is_empty());
    assert_eq!(reason_of(&batch, "kfd"), Some("refused"));
    assert_eq!(agent.query_requests().await.len(), 2, "no third attempt");
}

#[tokio::test]
async fn a_403_is_refused_and_the_connection_is_outside_the_audience() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent.query(ResponseTemplate::new(403)).await;
    let batch = first_batch(&fx).await;
    assert_eq!(reason_of(&batch, "kfd"), Some("refused"));
    assert_eq!(state_of(&fx).await, "not_in_audience");
    let live = fx.client.connections_of(fx.ws).await;
    assert_eq!(live[0].state, ConnectionState::NotInAudience);
    // It is no longer asked.
    let ask = fx.ask(AskTarget::Workspace(fx.ws), "question");
    assert!(matches!(
        fx.client.ask(ask).await,
        Err(KqError::NoConnections)
    ));
}

#[tokio::test]
async fn a_sign_in_that_cannot_be_had_is_refused_and_flagged() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    *fx.tokens.audience_mode.lock().unwrap() = AudienceMode::Permanent;
    let batch = first_batch(&fx).await;
    assert_eq!(reason_of(&batch, "kfd"), Some("refused"));
    assert_eq!(state_of(&fx).await, "sign_in_needed");
    assert!(agent.query_requests().await.is_empty());
    // A temporary failure of the sign-in service is unreachable.
    let (fx, _agent) = connected("kfd.example.org", "kfd").await;
    *fx.tokens.audience_mode.lock().unwrap() = AudienceMode::Retryable;
    let batch = first_batch(&fx).await;
    assert_eq!(reason_of(&batch, "kfd"), Some("unreachable"));
}

#[tokio::test]
async fn a_409_loop_drops_the_agent_silently_but_another_409_is_unreachable() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent
        .query(
            ResponseTemplate::new(409).set_body_json(json!({ "error": "loop", "code": "KQ_LOOP" })),
        )
        .await;
    let batch = first_batch(&fx).await;
    assert!(batch.unanswered.is_empty() && batch.answered.is_empty());
    assert!(batch.final_batch);

    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent
        .query(ResponseTemplate::new(409).set_body_string("conflict"))
        .await;
    assert_eq!(
        reason_of(&first_batch(&fx).await, "kfd"),
        Some("unreachable")
    );
}

#[tokio::test]
async fn a_server_error_or_a_dead_connection_is_unreachable() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent.query(ResponseTemplate::new(500)).await;
    let batch = first_batch(&fx).await;
    assert_eq!(reason_of(&batch, "kfd"), Some("unreachable"));
    assert_eq!(state_of(&fx).await, "unreachable");
    assert_eq!(agent.query_requests().await.len(), 1, "no retry");
    // An unreachable connection is still asked, and recovers when it answers.
    agent.mock.reset().await;
    agent.answers(&[hit("kfd", "a.md", "back again")]).await;
    assert_eq!(first_batch(&fx).await.hits.len(), 1);
    assert_eq!(state_of(&fx).await, "ready");

    // Unix only: Windows retries a refused connection for about two seconds, past the deadline.
    #[cfg(unix)]
    {
        fx.client.inner.transport.rewrites.lock().unwrap().insert(
            "https://kfd.example.org".into(),
            "http://127.0.0.1:1".into(),
        );
        assert_eq!(
            reason_of(&first_batch(&fx).await, "kfd"),
            Some("unreachable")
        );
    }
}

#[tokio::test]
async fn an_event_that_is_not_valid_for_its_name_makes_the_agent_unreachable() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    for body in [
        sse(&[("result", json!({ "hits": "not a list" }))]),
        sse(&[("done", json!({ "unanswered": 7 }))]),
        sse(&[("more_found", json!({ "nope": true }))]),
    ] {
        agent.mock.reset().await;
        agent.query(sse_template(body)).await;
        let batch = first_batch(&fx).await;
        assert_eq!(reason_of(&batch, "kfd"), Some("unreachable"));
    }
}

#[tokio::test]
async fn an_unknown_event_is_ignored_and_comment_lines_do_nothing() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    let body = format!(
        ": keep-alive\n\nevent: progress\ndata: {{\"x\":1}}\n\n{}",
        sse(&[("result", result_event(&[hit("kfd", "a.md", "a fact")]))])
    );
    agent.query(sse_template(body)).await;
    assert_eq!(first_batch(&fx).await.hits.len(), 1);
}

#[tokio::test]
async fn more_found_and_done_extend_the_answer_and_every_hit_gets_the_answer_id() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    let answer = KnowledgeAnswerId::new();
    let mut carried = hit("kfd", "a.md", "first fact");
    carried.answer_id = Some(KnowledgeAnswerId::new()); // never trusted from a hit
    let mut result = result_event(&[carried]);
    result["answer_id"] = json!(answer);
    result["more_expected"] = json!(true);
    result["unanswered"] = json!([{ "agent": "deep-proxy", "reason": "refused" }]);
    let body = sse(&[
        ("result", result),
        (
            "more_found",
            json!({ "hits": [hit("kfd", "b.md", "second fact")], "unanswered": [] }),
        ),
        (
            "done",
            json!({ "unanswered": [{ "agent": "deep-proxy", "reason": "refused" }], "unchecked_sources": true }),
        ),
    ]);
    agent.query(sse_template(body)).await;

    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "question"))
        .await
        .unwrap();
    let first = stream.first().await;
    assert!(!first.final_batch);
    assert_eq!(first.hits.len(), 1);
    assert_eq!(first.hits[0].answer_id, Some(answer));
    assert_eq!(first.answered, ["kfd"]);
    assert_eq!(reason_of(&first, "deep-proxy"), Some("refused"));
    assert_eq!(
        first.relied_on,
        [(
            "kfd".to_owned(),
            "git.example.org/supply/handbook@main".to_owned(),
            COMMIT.to_owned()
        )]
    );

    let mut all_hits = first.hits.len();
    let mut last = first;
    let mut saw_more = false;
    while let Some(batch) = stream.more().await {
        saw_more |= batch.hits.len() == 2;
        all_hits = all_hits.max(batch.hits.len());
        assert!(batch.hits.iter().all(|h| h.answer_id == Some(answer)));
        last = batch;
    }
    assert!(saw_more);
    assert_eq!(all_hits, 2);
    assert!(last.final_batch);
    assert!(last.unchecked_sources);
    assert_eq!(
        last.unanswered
            .iter()
            .filter(|u| u.agent == "deep-proxy")
            .count(),
        1,
        "de-duplicated"
    );
}

#[tokio::test]
async fn dropping_the_stream_cancels_every_open_task() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent
        .query(
            sse_template(sse(&[("result", result_event(&[]))])).set_delay(Duration::from_secs(10)),
        )
        .await;
    let stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "question"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        Arc::strong_count(&fx.client.inner) > 1,
        "a task holds the client's state"
    );
    drop(stream);
    eventually("the tasks to end", || async {
        Arc::strong_count(&fx.client.inner) == 1
    })
    .await;
}

#[tokio::test]
async fn closing_the_client_ends_an_ask_in_flight_with_its_sources_unreachable() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent
        .query(
            sse_template(sse(&[("result", result_event(&[]))])).set_delay(Duration::from_secs(10)),
        )
        .await;
    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "question"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    fx.client.close().await;
    let batch = stream.first().await;
    assert!(batch.final_batch);
    assert_eq!(reason_of(&batch, "kfd"), Some("unreachable"));
    assert!(stream.more().await.is_none());
}

#[tokio::test]
async fn closing_during_more_found_ends_the_stream_too() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    let mut result = result_event(&[hit("kfd", "a.md", "a fact")]);
    result["more_expected"] = json!(true);
    agent.query(sse_template(sse(&[("result", result)]))).await;
    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "question"))
        .await
        .unwrap();
    let first = stream.first().await;
    assert!(!first.final_batch);
    fx.client.close().await;
    let mut last = first;
    while let Some(batch) = stream.more().await {
        last = batch;
    }
    assert!(last.final_batch);
}

#[tokio::test]
async fn searches_travel_as_typed_sub_queries() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent.answers(&[]).await;
    let mut ask = fx.ask(AskTarget::Workspace(fx.ws), "unused");
    ask.query = None;
    ask.searches = Some(vec![
        ordin_types::wire::knowledge_query::KnowledgeSearch {
            kind: ordin_types::wire::knowledge_query::KnowledgeSearchKind::Lex,
            query: "packaging".into(),
        },
        ordin_types::wire::knowledge_query::KnowledgeSearch {
            kind: ordin_types::wire::knowledge_query::KnowledgeSearchKind::Vec,
            query: "who supplies boxes".into(),
        },
    ]);
    let mut stream = fx.client.ask(ask).await.unwrap();
    let _ = stream.first().await;
    let body = body_of(&agent.query_requests().await[0]);
    assert!(body.get("query").is_none());
    assert_eq!(
        body["searches"],
        json!([{ "type": "lex", "query": "packaging" }, { "type": "vec", "query": "who supplies boxes" }])
    );
}

#[tokio::test]
async fn the_ask_log_line_holds_counts_and_timings_only() {
    let capture = LogCapture::start();
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent
        .answers(&[hit("kfd", "a.md", "a very private fact")])
        .await;
    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "a very private question"))
        .await
        .unwrap();
    let _ = stream.first().await;
    eventually("the kq_ask line", || async {
        capture.text().contains("kq_ask")
    })
    .await;
    let log = capture.text();
    assert!(!log.contains("a very private"));
    assert!(!log.contains("kfd.example.org"));
}

// ---- delegation

#[tokio::test]
async fn a_delegation_that_cannot_be_issued_is_not_sent() {
    let fx = Fixture::new().await;
    let mut about = about_json("kfd", ISSUER, "aud-kfd");
    about["sharepoint_check_scope"] = json!("api://app/knowledge.check");
    let agent = fx.stand_in("kfd.example.org", "kfd").await;
    agent.about(about).await;
    agent.contents(contents_json("kfd")).await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    *fx.tokens.scope_ok.lock().unwrap() = false;
    agent.answers(&[hit("kfd", "a.md", "a fact")]).await;
    let batch = first_batch(&fx).await;
    assert_eq!(batch.hits.len(), 1);
    assert!(body_of(&agent.query_requests().await[0])
        .get("delegation")
        .is_none());
}

// ---- the agent profile: service token and hedge

async fn agent_asks(fx: &Fixture, names: &[&str]) -> KnowledgeBatch {
    let target = names
        .iter()
        .map(|n| ProxyAsk {
            primary: (*n).into(),
            backup: None,
        })
        .collect();
    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Proxies(target), "question"))
        .await
        .unwrap();
    stream.first().await
}

async fn sts(fx: &Fixture, template: ResponseTemplate) -> Agent {
    let sts = fx.stand_in("sts.example.org", "sts").await;
    sts.route("POST", "/token", template).await;
    sts
}

fn token_ok() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(json!({ "access_token": "svc-token", "expires_in": 3600 }))
}

#[tokio::test]
async fn the_service_token_is_a_client_credentials_grant_cached_per_proxy() {
    let fx = Fixture::agent_mode(&["p1", "p2"]).await;
    let sts = sts(&fx, token_ok()).await;
    let mut keep = Vec::new();
    for name in ["p1", "p2"] {
        let p = fx.stand_in(&format!("{name}.example.org"), name).await;
        p.answers(&[hit(name, &format!("{name}.md"), &format!("fact of {name}"))])
            .await;
        keep.push(p);
    }
    let first = agent_asks(&fx, &["p1", "p2"]).await;
    assert_eq!(first.hits.len(), 2);
    let second = agent_asks(&fx, &["p1"]).await;
    assert_eq!(second.hits.len(), 1);
    // One grant per proxy; the second ask of p1 used the cached token.
    assert_eq!(sts.count("POST", "/token").await, 2);
    let form = String::from_utf8(sts.requests().await[0].body.clone()).unwrap();
    let pairs: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(form.as_bytes())
            .into_owned()
            .collect();
    assert_eq!(pairs["grant_type"], "client_credentials");
    assert_eq!(pairs["client_id"], "front-door");
    assert_eq!(pairs["client_secret"], "s3cret");
    assert_eq!(pairs["scope"], "knowledge");
    assert!(pairs["audience"].starts_with("aud-p"));
}

#[tokio::test]
async fn a_refused_grant_refuses_every_proxy_and_is_logged() {
    let capture = LogCapture::start();
    let fx = Fixture::agent_mode(&["p1", "p2"]).await;
    let _sts = sts(&fx, ResponseTemplate::new(401)).await;
    let p1 = fx.stand_in("p1.example.org", "p1").await;
    let _p2 = fx.stand_in("p2.example.org", "p2").await;
    let batch = agent_asks(&fx, &["p1", "p2"]).await;
    assert_eq!(reason_of(&batch, "p1"), Some("refused"));
    assert_eq!(reason_of(&batch, "p2"), Some("refused"));
    assert!(p1.query_requests().await.is_empty());
    assert!(capture.text().contains("kq_service_token_refused"));
    assert!(!capture.text().contains("s3cret"));
}

#[tokio::test]
async fn a_token_endpoint_that_fails_makes_the_proxies_unreachable() {
    // No token endpoint mapped at all: the connection is refused. Unix only: Windows retries a
    // refused connection for about two seconds, past the deadline.
    #[cfg(unix)]
    {
        let fx = Fixture::agent_mode(&["p1"]).await;
        let _p1 = fx.stand_in("p1.example.org", "p1").await;
        fx.client.inner.transport.rewrites.lock().unwrap().insert(
            "https://sts.example.org".into(),
            "http://127.0.0.1:1".into(),
        );
        assert_eq!(
            reason_of(&agent_asks(&fx, &["p1"]).await, "p1"),
            Some("unreachable")
        );
    }
    for template in [
        ResponseTemplate::new(500),
        ResponseTemplate::new(200).set_body_string("not json"),
        ResponseTemplate::new(200).set_body_json(json!({ "expires_in": 5 })),
    ] {
        let fx = Fixture::agent_mode(&["p1"]).await;
        let _p1 = fx.stand_in("p1.example.org", "p1").await;
        let _sts = sts(&fx, template).await;
        assert_eq!(
            reason_of(&agent_asks(&fx, &["p1"]).await, "p1"),
            Some("unreachable")
        );
    }
}

#[tokio::test]
async fn a_proxy_401_drops_the_cached_service_token_and_retries_once() {
    let fx = Fixture::agent_mode(&["p1"]).await;
    let sts = sts(&fx, token_ok()).await;
    let p1 = fx.stand_in("p1.example.org", "p1").await;
    Mock::given(method("POST"))
        .and(path("/v1/knowledge/query"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&p1.mock)
        .await;
    p1.answers(&[hit("p1", "a.md", "a fact")]).await;
    let batch = agent_asks(&fx, &["p1"]).await;
    assert_eq!(batch.hits.len(), 1);
    assert_eq!(
        sts.count("POST", "/token").await,
        2,
        "a fresh grant for the retry"
    );
}

#[tokio::test]
async fn the_backup_is_not_asked_when_the_primary_answers_in_time() {
    let fx = Fixture::agent_mode(&["p1", "p2"]).await;
    let _sts = sts(&fx, token_ok()).await;
    let p1 = fx.stand_in("p1.example.org", "p1").await;
    let p2 = fx.stand_in("p2.example.org", "p2").await;
    p1.answers(&[hit("p1", "a.md", "a fact")]).await;
    p2.answers(&[hit("p2", "b.md", "never asked")]).await;
    let mut stream = fx
        .client
        .ask(fx.ask(
            AskTarget::Proxies(vec![ProxyAsk {
                primary: "p1".into(),
                backup: Some("p2".into()),
            }]),
            "question",
        ))
        .await
        .unwrap();
    let batch = stream.first().await;
    assert!(batch.final_batch);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(p2.query_requests().await.is_empty());
}

#[tokio::test]
async fn when_neither_of_a_pair_answers_both_are_listed() {
    let fx = Fixture::agent_mode(&["p1", "p2"]).await;
    let _sts = sts(&fx, token_ok()).await;
    let p1 = fx.stand_in("p1.example.org", "p1").await;
    let p2 = fx.stand_in("p2.example.org", "p2").await;
    p1.query(ResponseTemplate::new(500)).await;
    p2.query(ResponseTemplate::new(403)).await;
    let mut stream = fx
        .client
        .ask(fx.ask(
            AskTarget::Proxies(vec![ProxyAsk {
                primary: "p1".into(),
                backup: Some("p2".into()),
            }]),
            "question",
        ))
        .await
        .unwrap();
    let batch = stream.first().await;
    assert!(batch.final_batch);
    assert_eq!(reason_of(&batch, "p1"), Some("unreachable"));
    assert_eq!(reason_of(&batch, "p2"), Some("refused"));
}

#[tokio::test]
async fn a_visited_backup_is_not_asked_and_a_loop_from_a_proxy_is_dropped() {
    let fx = Fixture::agent_mode(&["p1", "p2"]).await;
    let _sts = sts(&fx, token_ok()).await;
    let p1 = fx.stand_in("p1.example.org", "p1").await;
    let p2 = fx.stand_in("p2.example.org", "p2").await;
    p1.query(ResponseTemplate::new(409).set_body_json(json!({ "code": "KQ_LOOP" })))
        .await;
    p2.answers(&[]).await;
    let mut ask = fx.ask(
        AskTarget::Proxies(vec![ProxyAsk {
            primary: "p1".into(),
            backup: Some("p2".into()),
        }]),
        "question",
    );
    ask.visited = vec!["p2".to_owned()];
    let mut stream = fx.client.ask(ask).await.unwrap();
    let batch = stream.first().await;
    assert!(batch.unanswered.is_empty(), "{:?}", batch.unanswered);
    assert!(p2.query_requests().await.is_empty());
    assert_eq!(
        body_of(&p1.query_requests().await[0])["visited"],
        json!(["p2"])
    );
}

// ---- connections

#[tokio::test]
async fn removing_a_person_connection_deletes_it_with_its_contents() {
    let (fx, _agent) = connected("kfd.example.org", "kfd").await;
    let id = fx.client.list_connections(fx.ws).await.unwrap()[0].connection_id;
    fx.client.remove_connection(id).await.unwrap();
    use sqlx::Row;
    for table in ["kq_connections", "kq_connection_contents"] {
        let n: i64 = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) AS n FROM {table}"
        )))
        .fetch_one(&fx.pool())
        .await
        .unwrap()
        .get("n");
        assert_eq!(n, 0, "{table}");
    }
    assert_eq!(
        fx.event_kinds().await,
        ["knowledge_agents_changed", "knowledge_agents_changed"]
    );
    let kinds = fx.journal_kinds().await;
    assert!(
        kinds.contains(&"knowledge_agent_disconnected".to_owned()),
        "{kinds:?}"
    );
}

#[tokio::test]
async fn details_read_the_front_door_afresh() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    let id = fx.client.list_connections(fx.ws).await.unwrap()[0].connection_id;
    // The front door has since fetched another repository.
    agent.mock.reset().await;
    agent.about(about_json("kfd", ISSUER, "aud-kfd")).await;
    let mut contents = contents_json("kfd");
    contents["repositories"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "repository": "git.example.org/supply/glossary@main",
            "role": "glossary",
            "last_commit": COMMIT,
            "last_fetched_at": "2026-01-10T07:00:00.000Z",
            "readable": true,
            "fact_count": 1
        }));
    agent.contents(contents).await;
    let details = fx.client.details(id).await.unwrap();
    assert_eq!(details.repositories.len(), 2);
    assert_eq!(details.state, ConnectionState::Ready);
    assert!(details.about.is_some());
    assert_eq!(details.last_problem, None);
    assert_eq!(
        details.as_of.map(|t| t.to_string()),
        Some("2026-01-10T07:00:00.000Z".to_owned()),
        "the oldest last fetch"
    );
}

#[tokio::test]
async fn details_of_an_unreachable_front_door_return_the_stored_values() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    let id = fx.client.list_connections(fx.ws).await.unwrap()[0].connection_id;
    agent.mock.reset().await;
    agent
        .route("GET", "/v1/knowledge/about", ResponseTemplate::new(503))
        .await;
    let details = fx.client.details(id).await.unwrap();
    assert!(details.about.is_none());
    assert_eq!(details.repositories.len(), 1, "the stored contents");
    assert_eq!(details.state, ConnectionState::Ready, "the row's state");
    let problem = details.last_problem.expect("a sentence");
    assert!(
        problem.starts_with("The knowledge agent could not be reached at "),
        "{problem}"
    );
    assert!(problem.chars().count() <= 300);

    // Each failure has its sentence.
    for (error, sentence) in [
        (
            KqError::SignInNeeded,
            "Sign in to your company account again to use this knowledge agent.",
        ),
        (
            KqError::NotInAudience,
            "This knowledge agent serves another group of people.",
        ),
        (
            KqError::OtherCompany,
            "This knowledge agent accepts another company's sign-in.",
        ),
    ] {
        assert_eq!(crate::client::problem_sentence(&error), sentence);
    }
}

#[tokio::test]
async fn an_issuer_with_a_trailing_slash_is_the_same_issuer() {
    let fx = Fixture::new().await;
    let agent = fx.stand_in("kfd.example.org", "kfd").await;
    agent
        .about(about_json("kfd", "https://id.example.org/", "aud-kfd"))
        .await;
    agent.contents(contents_json("kfd")).await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .expect("same issuer");
}

#[tokio::test]
async fn company_reconciliation_adds_removes_and_skips() {
    let fx = Fixture::new().await;
    let _a = fx.agent("a.example.org", "agent-a").await;
    let _b = fx.agent("b.example.org", "agent-b").await;
    // A person-added front door at an address a company entry also names.
    fx.client
        .add_connection(fx.ws, "https://b.example.org")
        .await
        .unwrap();
    let other = ordin_types::WorkspaceId::new();
    fx.client
        .apply_company_front_doors(vec![
            (
                fx.ws,
                vec![
                    company("a", "https://a.example.org"),
                    company("b", "https://b.example.org"),
                    company("bad", "http://not-https.example.org"),
                ],
            ),
            (other, vec![company("a", "https://a.example.org")]),
        ])
        .await
        .unwrap();
    let views = fx.client.list_connections(fx.ws).await.unwrap();
    assert_eq!(
        views.len(),
        2,
        "b was already connected; the bad address is skipped"
    );

    // The background read replaces the placeholder issuer, audience and state.
    eventually("the company row to be ready", || async {
        fx.client
            .connections_of(fx.ws)
            .await
            .iter()
            .all(|c| c.state == ConnectionState::Ready)
    })
    .await;
    use sqlx::Row;
    let row = sqlx::query("SELECT token_audience, issuer, sources_record_enabled FROM kq_connections WHERE company_entry_key = 'a' AND workspace_id = ?1")
        .bind(fx.ws.to_string())
        .fetch_one(&fx.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("token_audience"), "aud-agent-a");
    assert_eq!(row.get::<String, _>("issuer"), ISSUER);
    assert_eq!(row.get::<i64, _>("sources_record_enabled"), 1);

    // An entry the profile stops naming is deleted; one workspace's change does not touch another's.
    fx.client
        .apply_company_front_doors(vec![(fx.ws, vec![])])
        .await
        .unwrap();
    assert_eq!(fx.client.list_connections(fx.ws).await.unwrap().len(), 1);
    assert_eq!(fx.client.list_connections(other).await.unwrap().len(), 1);
    // Reconciling twice changes nothing and raises no second event.
    let events = fx.event_kinds().await.len();
    fx.client
        .apply_company_front_doors(vec![(fx.ws, vec![])])
        .await
        .unwrap();
    assert_eq!(fx.event_kinds().await.len(), events);
}

#[tokio::test]
async fn a_company_front_door_that_cannot_be_read_waits_unreachable_with_a_sentence() {
    let fx = Fixture::new().await;
    let down = fx.stand_in("down.example.org", "down").await;
    down.route("GET", "/v1/knowledge/about", ResponseTemplate::new(500))
        .await;
    fx.client
        .apply_company_front_doors(vec![(
            fx.ws,
            vec![company("down", "https://down.example.org")],
        )])
        .await
        .unwrap();
    eventually("the failure to be recorded", || async {
        use sqlx::Row;
        let row = sqlx::query("SELECT last_problem FROM kq_connections")
            .fetch_one(&fx.pool())
            .await
            .unwrap();
        row.get::<Option<String>, _>("last_problem").is_some()
    })
    .await;
    let live = fx.client.connections_of(fx.ws).await;
    assert_eq!(live[0].state, ConnectionState::Unreachable);
    assert_eq!(live[0].added_by, ordin_types::common::SetBy::Company);
}

#[tokio::test]
async fn contents_names_are_de_duplicated_across_connections_and_capped() {
    let fx = Fixture::new().await;
    let a = fx.stand_in("a.example.org", "a").await;
    a.about(about_json("a", ISSUER, "aud-a")).await;
    let mut contents = contents_json("a");
    contents["things"] = json!((0..3_000)
        .map(|n| json!({ "name": format!("Thing {n}"), "aliases": [format!("Alias {n}"), "Borealis"], "link_count": 1 }))
        .collect::<Vec<_>>());
    a.contents(contents).await;
    let b = fx.agent("b.example.org", "b").await;
    let _ = &b;
    fx.client
        .add_connection(fx.ws, "https://a.example.org")
        .await
        .unwrap();
    fx.client
        .add_connection(fx.ws, "https://b.example.org")
        .await
        .unwrap();
    let names = fx.client.contents_names(fx.ws).await;
    assert_eq!(names.len(), 4_000, "capped");
    assert_eq!(names.iter().filter(|n| *n == "Borealis").count(), 1);
    // A workspace with no connection has no names.
    assert!(fx
        .client
        .contents_names(ordin_types::WorkspaceId::new())
        .await
        .is_empty());
}

#[tokio::test]
async fn contents_over_two_mebibytes_are_stored_without_their_things() {
    let capture = LogCapture::start();
    let fx = Fixture::new().await;
    let a = fx.stand_in("a.example.org", "a").await;
    a.about(about_json("a", ISSUER, "aud-a")).await;
    let mut contents = contents_json("a");
    let long = "x".repeat(90);
    contents["things"] = json!((0..24_000)
        .map(|n| json!({ "name": format!("{long} {n}"), "aliases": [], "link_count": 1 }))
        .collect::<Vec<_>>());
    a.contents(contents).await;
    fx.client
        .add_connection(fx.ws, "https://a.example.org")
        .await
        .unwrap();
    assert!(fx.client.contents_names(fx.ws).await.is_empty());
    use sqlx::Row;
    let json: String = sqlx::query("SELECT contents_json FROM kq_connection_contents")
        .fetch_one(&fx.pool())
        .await
        .unwrap()
        .get("contents_json");
    assert!(json.len() < 2 * 1024 * 1024);
    let stored: KnowledgeContents = serde_json::from_str(&json).unwrap();
    assert!(stored.things.is_empty());
    assert_eq!(stored.repositories.len(), 1);
    assert!(capture.text().contains("kq_contents_trimmed"));
}

#[tokio::test]
async fn an_agent_has_no_connections_and_no_check_scopes() {
    let fx = Fixture::agent_mode(&["p1"]).await;
    assert!(fx.client.connections_of(fx.ws).await.is_empty());
    assert!(fx.client.microsoft_check_scopes().await.is_empty());
    assert!(fx.client.proxy_contents().is_empty());
}

// ---- the background tasks

#[tokio::test]
async fn the_start_up_pass_refreshes_every_connection_and_links_it() {
    let fx = Fixture::with_timings(Timings {
        refresh_interval: Duration::from_millis(80),
        ..quick_timings()
    })
    .await;
    fx.tokens
        .set_credentials(vec![credential(CredentialKind::Github, "github.com")]);
    let agent = fx.stand_in("kfd.example.org", "kfd").await;
    let mut about = about_json("kfd", ISSUER, "aud-kfd");
    about["accounts"] =
        json!([{ "system": "github", "host": "github.com" }, { "system": "ldap", "host": "x" }]);
    agent.about(about).await;
    agent.contents(contents_json("kfd")).await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            ResponseTemplate::new(200)
                .set_body_json(json!({ "linked": ["github"], "refused": [] })),
        )
        .await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    let posts_after_add = agent.count("POST", "/v1/knowledge/accounts").await;
    assert_eq!(posts_after_add, 1);
    sqlx::query("UPDATE kq_connections SET state = 'unreachable'")
        .execute(&fx.pool())
        .await
        .unwrap();

    fx.client.inner.start_maintenance();
    eventually("the refresh", || async { state_of(&fx).await == "ready" }).await;
    eventually("the link at start", || async {
        agent.count("POST", "/v1/knowledge/accounts").await == posts_after_add + 1
    })
    .await;
    // And again at every interval.
    eventually("a second refresh", || async {
        agent.count("GET", "/v1/knowledge/about").await >= 3
    })
    .await;
    assert_eq!(
        agent.count("POST", "/v1/knowledge/accounts").await,
        posts_after_add + 1,
        "linking happens at start, not at every refresh"
    );
    fx.client.close().await;
}

#[tokio::test]
async fn a_failed_refresh_records_why() {
    let (fx, agent) = connected("kfd.example.org", "kfd").await;
    agent.mock.reset().await;
    agent
        .about(about_json("kfd", "https://id.other.example", "aud-kfd"))
        .await;
    fx.client.inner.start_maintenance();
    eventually("the sentence", || async {
        use sqlx::Row;
        sqlx::query("SELECT last_problem FROM kq_connections")
            .fetch_one(&fx.pool())
            .await
            .unwrap()
            .get::<Option<String>, _>("last_problem")
            .is_some()
    })
    .await;
    assert_eq!(state_of(&fx).await, "unreachable");
    fx.client.close().await;
}

#[tokio::test]
async fn a_lagged_sign_in_receiver_relinks_every_system_of_every_front_door() {
    let fx = Fixture::new().await;
    fx.tokens
        .set_credentials(vec![credential(CredentialKind::Github, "github.com")]);
    let agent = fx.stand_in("kfd.example.org", "kfd").await;
    let mut about = about_json("kfd", ISSUER, "aud-kfd");
    about["accounts"] = json!([{ "system": "github", "host": "github.com" }]);
    agent.about(about).await;
    agent.contents(contents_json("kfd")).await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            ResponseTemplate::new(200)
                .set_body_json(json!({ "linked": [{"system":"github"}], "refused": [] })),
        )
        .await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    fx.client.inner.start_sign_in_loop();
    eventually("the listener", || async {
        fx.tokens.changes.receiver_count() > 0
    })
    .await;
    // Ten changes with no await between them overflow the channel of four.
    for _ in 0..10 {
        let _ = fx.tokens.changes.send(ordin_identity::SignInChanged {
            credential_id: ordin_types::CredentialId::new(),
            credential_kind: CredentialKind::Slack,
            state: ordin_identity::SignInState::SignedIn,
        });
    }
    eventually("the relink", || async {
        agent.count("POST", "/v1/knowledge/accounts").await == 2
    })
    .await;
    // Closing the broadcast ends the listener.
    fx.client.close().await;
}

#[tokio::test]
async fn linking_edge_cases() {
    // A credential for another host, or one that needs a sign-in, is not used; a failed Confluence
    // token read leaves it out silently; a front door that no longer checks a system drops its row.
    let fx = Fixture::new().await;
    let mut needs_sign_in = credential(CredentialKind::Github, "github.com");
    needs_sign_in.needs_sign_in = true;
    fx.tokens.set_credentials(vec![
        needs_sign_in,
        credential(CredentialKind::Gitlab, "gitlab.example.org"),
        credential(CredentialKind::Confluence, "acme.atlassian.net"),
    ]);
    *fx.tokens.confluence_ok.lock().unwrap() = false;
    let agent = fx.stand_in("kfd.example.org", "kfd").await;
    let mut about = about_json("kfd", ISSUER, "aud-kfd");
    about["accounts"] = json!([
        { "system": "github", "host": "github.com" },
        { "system": "gitlab", "host": "gitlab.other.example" },
        { "system": "confluence", "host": "acme.atlassian.net" }
    ]);
    agent.about(about).await;
    agent.contents(contents_json("kfd")).await;
    let (view, _) = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    assert_eq!(
        agent.count("POST", "/v1/knowledge/accounts").await,
        0,
        "nothing to send"
    );
    let details = fx.client.details(view.connection_id).await.unwrap();
    assert!(details
        .accounts
        .iter()
        .all(|a| a.state == AccountLinkState::NotSignedIn));
    assert_eq!(details.accounts.len(), 3);

    // The Confluence token comes back; a GitHub credential matches on its host.
    *fx.tokens.confluence_ok.lock().unwrap() = true;
    fx.tokens.set_credentials(vec![
        credential(CredentialKind::Github, "github.com"),
        credential(CredentialKind::Confluence, "acme.atlassian.net"),
    ]);
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            ResponseTemplate::new(200).set_body_json(json!({
                "linked": [{ "system": "github" }],
                "refused": [
                    { "system": "confluence", "code": "KQ_ACCOUNT_LINK_REFUSED", "reason": "not_checked_here" },
                    { "system": "github", "code": "KQ_ACCOUNT_LINK_REFUSED", "reason": "something_new" }
                ]
            })),
        )
        .await;
    let row = crate::store::get(&fx.pool(), view.connection_id)
        .await
        .unwrap()
        .unwrap();
    fx.client.inner.link_connection(&row, None).await;
    let body = body_of(
        &agent
            .requests()
            .await
            .into_iter()
            .find(|r| r.url.path() == "/v1/knowledge/accounts")
            .unwrap(),
    );
    let systems: Vec<&str> = body["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["system"].as_str().unwrap())
        .collect();
    assert_eq!(systems, ["github", "confluence"]);
    let accounts = crate::store::accounts_of(&fx.pool(), view.connection_id)
        .await
        .unwrap();
    assert!(
        accounts.iter().all(|(s, _)| *s != SourceSystem::Confluence),
        "the row is deleted"
    );
    assert!(accounts.contains(&(SourceSystem::Github, AccountLinkState::Linked)));
}

#[tokio::test]
async fn linking_retries_a_401_once_and_leaves_states_alone_when_the_front_door_is_down() {
    let fx = Fixture::new().await;
    fx.tokens
        .set_credentials(vec![credential(CredentialKind::Github, "github.com")]);
    let agent = fx.stand_in("kfd.example.org", "kfd").await;
    let mut about = about_json("kfd", ISSUER, "aud-kfd");
    about["accounts"] = json!([{ "system": "github", "host": "github.com" }]);
    agent.about(about).await;
    agent.contents(contents_json("kfd")).await;
    Mock::given(method("POST"))
        .and(path("/v1/knowledge/accounts"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&agent.mock)
        .await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            ResponseTemplate::new(200)
                .set_body_json(json!({ "linked": [{"system":"github"}], "refused": [] })),
        )
        .await;
    let (view, _) = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    assert_eq!(agent.count("POST", "/v1/knowledge/accounts").await, 2);
    assert_eq!(fx.tokens.invalidated.load(Ordering::SeqCst), 1);
    let accounts = crate::store::accounts_of(&fx.pool(), view.connection_id)
        .await
        .unwrap();
    assert_eq!(accounts, [(SourceSystem::Github, AccountLinkState::Linked)]);

    // A failing front door changes nothing.
    agent.mock.reset().await;
    agent
        .route("POST", "/v1/knowledge/accounts", ResponseTemplate::new(500))
        .await;
    let row = crate::store::get(&fx.pool(), view.connection_id)
        .await
        .unwrap()
        .unwrap();
    fx.client.inner.link_connection(&row, None).await;
    let accounts = crate::store::accounts_of(&fx.pool(), view.connection_id)
        .await
        .unwrap();
    assert_eq!(accounts, [(SourceSystem::Github, AccountLinkState::Linked)]);
    // A reply that is not a link reply changes nothing either.
    agent.mock.reset().await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            ResponseTemplate::new(200).set_body_string("nope"),
        )
        .await;
    fx.client.inner.link_connection(&row, None).await;
    assert_eq!(
        crate::store::accounts_of(&fx.pool(), view.connection_id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_front_door_naming_a_graph_scope_is_refused() {
    let fx = Fixture::new().await;
    let agent = fx.stand_in("g.example.org", "g").await;
    let mut about = about_json("g", ISSUER, "aud");
    about["sharepoint_check_scope"] = json!("https://graph.microsoft.com/Files.ReadWrite.All");
    agent.about(about).await;
    agent.contents(contents_json("g")).await;
    let added = fx
        .client
        .add_connection(fx.ws, "https://g.example.org")
        .await;
    assert!(matches!(added, Err(KqError::NotAKnowledgeAgent)));
    assert!(fx.client.microsoft_check_scopes().await.is_empty());
}

#[tokio::test]
async fn microsoft_check_scopes_are_de_duplicated() {
    let fx = Fixture::new().await;
    for host in ["a.example.org", "b.example.org", "c.example.org"] {
        let agent = fx.stand_in(host, host).await;
        let mut about = about_json(host, ISSUER, "aud");
        if host != "c.example.org" {
            about["sharepoint_check_scope"] = json!("api://shared/knowledge.check");
        }
        agent.about(about).await;
        agent.contents(contents_json(host)).await;
        fx.client
            .add_connection(fx.ws, &format!("https://{host}"))
            .await
            .unwrap();
    }
    assert_eq!(
        fx.client.microsoft_check_scopes().await,
        ["api://shared/knowledge.check"]
    );
}

// ---- change streams

#[derive(Default)]
struct Recorder {
    moved: Mutex<Vec<(String, RepositoryMoved)>>,
    contents: Mutex<Vec<(String, KnowledgeContents)>>,
    unreachable: Mutex<Vec<String>>,
}

impl ChangeListener for Recorder {
    fn repository_moved(&self, proxy: &str, moved: RepositoryMoved) {
        self.moved.lock().unwrap().push((proxy.to_owned(), moved));
    }
    fn contents_refreshed(&self, proxy: &str, contents: KnowledgeContents) {
        self.contents
            .lock()
            .unwrap()
            .push((proxy.to_owned(), contents));
    }
    fn proxy_unreachable(&self, proxy: &str) {
        self.unreachable.lock().unwrap().push(proxy.to_owned());
    }
}

#[tokio::test]
async fn change_streams_report_moves_and_refresh_contents_and_reconnect() {
    let fx = Fixture::agent_mode(&["p1"]).await;
    let _sts = sts(&fx, token_ok()).await;
    let p1 = fx.stand_in("p1.example.org", "p1").await;
    p1.contents(contents_json("p1")).await;
    let moved = json!({ "repository": "git.example.org/supply/handbook@main", "commit": COMMIT });
    p1.route(
        "GET",
        "/v1/knowledge/changes",
        sse_template(format!(
            "event: unrelated\ndata: 1\n\n{}event: repository_moved\ndata: not json\n\n",
            sse(&[("repository_moved", moved)])
        )),
    )
    .await;
    let recorder = Arc::new(Recorder::default());
    let stop = CancellationToken::new();
    fx.client
        .start_change_streams(recorder.clone(), stop.clone());

    eventually("the move", || async {
        !recorder.moved.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(recorder.moved.lock().unwrap()[0].0, "p1");
    assert_eq!(recorder.moved.lock().unwrap()[0].1.commit, COMMIT);
    eventually("the contents", || async {
        recorder.contents.lock().unwrap().len() >= 2
    })
    .await;
    assert_eq!(fx.client.proxy_contents().len(), 1);
    assert_eq!(fx.client.proxy_contents()[0].agent_name, "p1");
    // The stream ends after its events; it is opened again.
    eventually("a reconnect", || async {
        p1.count("GET", "/v1/knowledge/changes").await >= 2
    })
    .await;
    stop.cancel();
}

#[tokio::test]
async fn a_proxy_that_cannot_be_reached_is_reported_after_the_second_failure_in_a_row() {
    let capture = LogCapture::start();
    let fx = Fixture::agent_mode(&["p1"]).await;
    let _sts = sts(&fx, token_ok()).await;
    let p1 = fx.stand_in("p1.example.org", "p1").await;
    p1.route("GET", "/v1/knowledge/changes", ResponseTemplate::new(500))
        .await;
    let recorder = Arc::new(Recorder::default());
    let stop = CancellationToken::new();
    fx.client
        .start_change_streams(recorder.clone(), stop.clone());
    eventually("the report", || async {
        !recorder.unreachable.lock().unwrap().is_empty()
    })
    .await;
    assert!(
        p1.count("GET", "/v1/knowledge/changes").await >= 2,
        "not reported after the first failure"
    );
    assert_eq!(recorder.unreachable.lock().unwrap()[0], "p1");
    assert!(capture.text().contains("kq_change_stream_closed"));
    fx.client.close().await;
}

#[tokio::test]
async fn a_change_stream_stops_when_asked_and_a_401_drops_the_token() {
    let fx = Fixture::agent_mode(&["p1"]).await;
    let sts = sts(&fx, token_ok()).await;
    let p1 = fx.stand_in("p1.example.org", "p1").await;
    p1.route("GET", "/v1/knowledge/changes", ResponseTemplate::new(401))
        .await;
    let recorder = Arc::new(Recorder::default());
    let stop = CancellationToken::new();
    fx.client
        .start_change_streams(recorder.clone(), stop.clone());
    eventually("two attempts", || async {
        p1.count("GET", "/v1/knowledge/changes").await >= 2
    })
    .await;
    assert!(
        sts.count("POST", "/token").await >= 2,
        "the token was dropped after the 401"
    );
    stop.cancel();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let attempts = p1.count("GET", "/v1/knowledge/changes").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        p1.count("GET", "/v1/knowledge/changes").await,
        attempts,
        "no more attempts"
    );
}

#[tokio::test]
async fn change_streams_do_nothing_on_the_desktop() {
    let fx = Fixture::new().await;
    let recorder = Arc::new(Recorder::default());
    fx.client
        .start_change_streams(recorder.clone(), CancellationToken::new());
    assert!(fx.client.proxy_contents().is_empty());
    assert!(recorder.moved.lock().unwrap().is_empty());
}
