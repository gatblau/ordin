//! X2: one test per Gherkin scenario of KnowledgeQueryClient.md (main spec and the ADR-115
//! amendment), named after the scenario.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_identity::{
    AccessToken, CredError, IdentityConfig, IdentityService, KeySource, Program,
    SecretBackendConfig, SignInChanged,
};
use ordin_types::domain::identity::CredentialKind;
use ordin_types::wire::knowledge_query::{
    KnowledgeHit, KnowledgeHitKind, PageStatus, PageTier, SharedIdentity,
};
use ordin_types::CredentialId;
use serde_json::{json, Value};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::error_table::company;
use super::harness::*;
use crate::tokens::{IdentityTokens, LinkCredential, TokenSource};
use crate::*;

const SCOPE: &str = "api://7d1c0000/knowledge.check";
/// Base64 of 32 bytes, the secrets file's key.
const BASE64_KEY: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";

// ---- the main specification

#[tokio::test]
async fn happy_path_a_person_connects_a_front_door() {
    let fx = Fixture::new().await;
    let _agent = fx.agent("kfd.example.org", "procurement-knowledge").await;
    let (view, about) = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .expect("connects");

    use sqlx::Row;
    let row = sqlx::query("SELECT added_by, workspace_id, state FROM kq_connections WHERE id = ?1")
        .bind(view.connection_id.to_string())
        .fetch_one(&fx.pool())
        .await
        .expect("a kq_connections row exists");
    assert_eq!(row.get::<String, _>("added_by"), "person");
    assert_eq!(row.get::<String, _>("workspace_id"), fx.ws.to_string());
    assert_eq!(row.get::<String, _>("state"), "ready");

    assert_eq!(fx.event_kinds().await, ["knowledge_agents_changed"]);
    let events = fx.journal.events_since(0, 10).await.unwrap();
    assert_eq!(events[0].data, json!({ "workspace_id": fx.ws }));

    let page = fx
        .journal
        .list_entries(ordin_journal::EntryFilter::default(), None)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    let entry = &page.items[0];
    assert_eq!(entry.kind, "knowledge_agent_connected");
    assert_eq!(entry.component, "knowledge_query_client");
    assert_eq!(entry.workspace_id, Some(fx.ws));
    assert_eq!(
        entry.data,
        json!({ "connection_id": view.connection_id, "added_by": "person" })
    );
    assert!(!entry.summary.contains("kfd.example.org"));

    let policy = view.sources_record.expect("a sources record is kept");
    assert!(policy.enabled);
    assert_eq!(policy.retention_days, 365);
    assert_eq!(view.name, "procurement-knowledge");
    assert_eq!(view.last_answered_at, None);
    assert_eq!(about.token_audience, "aud-procurement-knowledge");
    assert_eq!(about.sources_record.map(|p| p.retention_days), Some(365));
}

#[tokio::test]
async fn error_the_person_is_outside_the_audience() {
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
    let error = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap_err();
    assert_eq!(error, KqError::NotInAudience);
    assert!(fx.client.list_connections(fx.ws).await.unwrap().is_empty());
    assert!(fx.event_kinds().await.is_empty());
}

#[tokio::test]
async fn edge_a_workspace_with_no_front_door_asks_nothing() {
    let fx = Fixture::new().await;
    // A front door exists on loopback, but W has none connected: it must see no request.
    let agent = fx.agent("kfd.example.org", "procurement").await;

    assert!(fx.client.connections_of(fx.ws).await.is_empty());
    assert!(fx.client.contents_names(fx.ws).await.is_empty());
    let ask = fx.ask(AskTarget::Workspace(fx.ws), "who supplies packaging");
    assert!(matches!(
        fx.client.ask(ask).await,
        Err(KqError::NoConnections)
    ));
    assert!(
        agent.requests().await.is_empty(),
        "no request left the computer"
    );
}

#[tokio::test]
async fn privacy_only_the_question_and_its_filters_are_sent() {
    let fx = Fixture::new().await;
    let agent = fx.agent("kfd.example.org", "procurement").await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    agent
        .answers(&[hit("procurement", "a.md", "Borealis packs for Leeds")])
        .await;

    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "who supplies packaging"))
        .await
        .unwrap();
    let batch = stream.first().await;
    assert_eq!(batch.hits.len(), 1);

    let requests = agent.query_requests().await;
    assert_eq!(requests.len(), 1);
    let body = body_of(&requests[0]);
    let mut keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "deadline_ms",
            "hop_budget",
            "limit",
            "more_until_ms",
            "query",
            "request_id",
            "visited"
        ]
    );
    assert_eq!(body["query"], "who supplies packaging");
    assert_eq!(body["limit"], 20);
    assert_eq!(body["hop_budget"], 1);
    assert_eq!(body["visited"], json!([]));
    // 1,000 ms less the 50 ms allowance and the time already spent; 5,000 ms likewise.
    let deadline = body["deadline_ms"].as_u64().unwrap();
    let more = body["more_until_ms"].as_u64().unwrap();
    assert!((100..=950).contains(&deadline), "{deadline}");
    assert!((deadline..=4_950).contains(&more), "{more}");
    assert_eq!(header_of(&requests[0], "accept"), "text/event-stream");
    assert_eq!(
        header_of(&requests[0], "authorization"),
        format!("Bearer {}", fx.tokens.bearer("aud-procurement"))
    );
}

#[tokio::test]
async fn deadline_a_slow_agent_is_reported_and_its_answer_arrives_as_more_found() {
    let fx = Fixture::new().await;
    let a = fx.agent("a.example.org", "agent-a").await;
    let b = fx.agent("b.example.org", "agent-b").await;
    fx.client
        .add_connection(fx.ws, "https://a.example.org")
        .await
        .unwrap();
    fx.client
        .add_connection(fx.ws, "https://b.example.org")
        .await
        .unwrap();
    a.answers(&[hit("agent-a", "a.md", "fact from A")]).await;
    b.query(
        sse_template(sse(&[(
            "result",
            result_event(&[hit("agent-b", "b.md", "fact from B")]),
        )]))
        .set_delay(Duration::from_secs(3)),
    )
    .await;

    let started = Instant::now();
    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "question"))
        .await
        .unwrap();
    let first = stream.first().await;
    let waited = started.elapsed();
    assert!(
        waited < Duration::from_millis(1_300),
        "first() took {waited:?}"
    );
    assert!(
        waited >= Duration::from_millis(900),
        "first() came early: {waited:?}"
    );
    assert_eq!(first.answered, ["agent-a"]);
    assert_eq!(first.hits.len(), 1);
    assert_eq!(first.hits[0].agent, "agent-a");
    assert_eq!(first.unanswered.len(), 1);
    assert_eq!(first.unanswered[0].agent, "agent-b");
    assert!(matches!(
        first.unanswered[0].reason,
        ordin_types::wire::knowledge_query::UnansweredReason::NotInTime
    ));
    assert!(!first.final_batch);

    let more = stream.more().await.expect("a later batch");
    assert!(started.elapsed() < Duration::from_secs(5));
    let agents: Vec<&str> = more.hits.iter().map(|h| h.agent.as_str()).collect();
    assert!(
        agents.contains(&"agent-a") && agents.contains(&"agent-b"),
        "{agents:?}"
    );
    assert!(more.unanswered.is_empty());

    let mut last = more;
    while !last.final_batch {
        last = stream.more().await.expect("the final batch");
    }
    assert!(last.final_batch);
    assert!(stream.more().await.is_none());
}

/// Stands up the agent profile's proxies and a token endpoint.
async fn proxies(names: &[&str]) -> (Fixture, Vec<Agent>) {
    let fx = Fixture::agent_mode(names).await;
    let sts = fx.stand_in("sts.example.org", "sts").await;
    sts.route(
        "POST",
        "/token",
        ResponseTemplate::new(200)
            .set_body_json(json!({ "access_token": "svc-token", "expires_in": 3600 })),
    )
    .await;
    let mut agents = vec![sts];
    for name in names {
        agents.push(fx.stand_in(&format!("{name}.example.org"), name).await);
    }
    (fx, agents)
}

#[tokio::test]
async fn backup_request_the_second_proxy_answers_when_the_first_is_slow() {
    let (fx, agents) = proxies(&["p1", "p2"]).await;
    let (p1, p2) = (&agents[1], &agents[2]);
    p1.query(
        sse_template(sse(&[(
            "result",
            result_event(&[hit("p1", "slow.md", "from the slow proxy")]),
        )]))
        .set_delay(Duration::from_secs(2)),
    )
    .await;
    p2.answers(&[hit("p2", "fast.md", "from the backup")]).await;

    let started = Instant::now();
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
    let first = stream.first().await;
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(300),
        "the backup waits 300 ms: {waited:?}"
    );
    assert!(waited < Duration::from_millis(1_000), "{waited:?}");
    assert_eq!(first.hits.len(), 1);
    assert_eq!(first.hits[0].agent, "p2");
    assert_eq!(first.answered, ["p2"]);
    assert!(first.unanswered.is_empty(), "{:?}", first.unanswered);
    assert_eq!(p1.query_requests().await.len(), 1);
    assert_eq!(p2.query_requests().await.len(), 1);
    // P1's task was cancelled: its late result never joins a batch.
    let mut last = first;
    while !last.final_batch {
        last = stream.more().await.expect("more or the end");
    }
    assert!(last.hits.iter().all(|h| h.agent == "p2"));
    assert!(last.unanswered.is_empty());
}

#[tokio::test]
async fn hop_loop_a_visited_agent_is_never_asked() {
    let (fx, agents) = proxies(&["p3"]).await;
    let mut ask = fx.ask(
        AskTarget::Proxies(vec![ProxyAsk {
            primary: "p3".into(),
            backup: None,
        }]),
        "question",
    );
    ask.visited = vec!["p3".to_owned()];
    let mut stream = fx.client.ask(ask).await.unwrap();
    let batch = stream.first().await;
    assert!(batch.hits.is_empty() && batch.unanswered.is_empty() && batch.final_batch);
    assert!(
        agents[1].requests().await.is_empty(),
        "no request is sent to p3"
    );
    assert!(stream.more().await.is_none());
}

fn equal_score_pair(
    first: (Option<PageTier>, KnowledgeHitKind),
    second: (Option<PageTier>, KnowledgeHitKind),
) -> Vec<KnowledgeHit> {
    let mut one = hit("p1", "one.md", "first fact");
    one.tier = first.0;
    one.kind = first.1;
    let mut two = hit("p2", "two.md", "second fact");
    two.tier = second.0;
    two.kind = second.1;
    // Each in its own list, at rank 1: equal fused scores.
    merge_hits(vec![("p1".into(), vec![one]), ("p2".into(), vec![two])], 20)
}

#[test]
fn merge_order_follows_trust_then_pages_then_rank() {
    use KnowledgeHitKind::{Document, Page};
    use PageTier::{HumanReviewed, MachineConfirmed, Unverified};
    // (a, b, c, d, first): the tier of the winner; "none" is None.
    let rows = [
        (
            Some(HumanReviewed),
            Page,
            Some(MachineConfirmed),
            Page,
            Some(HumanReviewed),
        ),
        (Some(Unverified), Page, None, Document, Some(Unverified)),
        (
            None,
            Document,
            Some(MachineConfirmed),
            Page,
            Some(MachineConfirmed),
        ),
    ];
    for (a, b, c, d, first) in rows {
        let merged = equal_score_pair((a, b), (c, d));
        assert_eq!(merged[0].tier, first, "{a:?} {b:?} against {c:?} {d:?}");
        // The order does not depend on which list came first.
        let swapped = equal_score_pair((c, d), (a, b));
        assert_eq!(swapped[0].tier, first, "swapped");
    }
}

#[test]
fn merge_the_same_fact_from_two_proxies_appears_once() {
    let mut from_first = hit("p1", "suppliers/borealis.md", "Borealis packs for Leeds");
    from_first.section = Some("Contract".to_owned());
    let mut from_second = from_first.clone();
    from_second.agent = "p2".to_owned();
    let merged = merge_hits(
        vec![
            ("p1".into(), vec![from_first]),
            ("p2".into(), vec![from_second]),
        ],
        20,
    );
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].agent, "p1");
}

#[test]
fn merge_deprecated_concepts_appear_only_when_nothing_else_matches() {
    let mut deprecated = hit("p1", "old.md", "the old rule");
    deprecated.status = PageStatus::Deprecated;
    let stable = hit("p1", "new.md", "the current rule");
    let merged = merge_hits(vec![("p1".into(), vec![deprecated, stable])], 20);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].statement, "the current rule");
}

#[test]
fn merge_facts_about_one_thing_are_grouped() {
    let identity = SharedIdentity {
        glossary: "git.example.org/glossary@main".into(),
        concept_path: "suppliers/borealis-ltd.md".into(),
    };
    let mut at_rank_one = hit("p1", "a.md", "Borealis is a packaging supplier");
    at_rank_one.shared_identity = Some(identity.clone());
    let mut at_rank_four = hit("p2", "d.md", "Borealis ships on Fridays");
    at_rank_four.shared_identity = Some(identity);
    let merged = merge_hits(
        vec![
            (
                "p1".into(),
                vec![
                    at_rank_one,
                    hit("p1", "b.md", "unrelated one"),
                    hit("p1", "c.md", "unrelated two"),
                    hit("p1", "e.md", "unrelated three"),
                ],
            ),
            (
                "p2".into(),
                vec![
                    hit("p2", "f.md", "other one"),
                    hit("p2", "g.md", "other two"),
                    hit("p2", "h.md", "other three"),
                    at_rank_four,
                ],
            ),
        ],
        20,
    );
    let at: Vec<usize> = merged
        .iter()
        .enumerate()
        .filter(|(_, h)| h.shared_identity.is_some())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(at.len(), 2);
    assert_eq!(at[1], at[0] + 1, "adjacent");
}

#[tokio::test]
async fn company_front_doors_a_removed_one_is_not_added_again() {
    let fx = Fixture::new().await;
    let agent = fx.agent("kfd.example.org", "procurement").await;
    let entry = company("hq-knowledge", "https://kfd.example.org");
    fx.client
        .apply_company_front_doors(vec![(fx.ws, vec![entry.clone()])])
        .await
        .unwrap();
    let views = fx.client.list_connections(fx.ws).await.unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].added_by, ordin_types::common::SetBy::Company);
    assert_eq!(views[0].name, "Company hq-knowledge");
    let id = views[0].connection_id;

    fx.client.remove_connection(id).await.unwrap();
    assert!(fx.client.list_connections(fx.ws).await.unwrap().is_empty());

    fx.client
        .apply_company_front_doors(vec![(fx.ws, vec![entry.clone()])])
        .await
        .unwrap();
    assert!(
        fx.client.list_connections(fx.ws).await.unwrap().is_empty(),
        "stays removed"
    );
    assert!(fx.client.connections_of(fx.ws).await.is_empty());
    // And it is not asked.
    let ask = fx.ask(AskTarget::Workspace(fx.ws), "question");
    assert!(matches!(
        fx.client.ask(ask).await,
        Err(KqError::NoConnections)
    ));
    assert!(agent.query_requests().await.is_empty());

    // The profile stops naming it, then names it again: it comes back (SA-KQC-06).
    fx.client
        .apply_company_front_doors(vec![(fx.ws, vec![])])
        .await
        .unwrap();
    fx.client
        .apply_company_front_doors(vec![(fx.ws, vec![entry])])
        .await
        .unwrap();
    assert_eq!(fx.client.list_connections(fx.ws).await.unwrap().len(), 1);
}

// ---- the ADR-115 amendment

/// Replies to `POST /v1/knowledge/accounts` by marking every system in the body as linked.
struct EchoLinked {
    account_id: Option<&'static str>,
}

impl Respond for EchoLinked {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let linked: Vec<Value> = body["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| match self.account_id {
                Some(id) => json!({ "system": a["system"], "account": id }),
                None => json!({ "system": a["system"] }),
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({ "linked": linked, "refused": [] }))
    }
}

fn about_with_accounts(name: &str) -> Value {
    let mut about = about_json(name, ISSUER, &format!("aud-{name}"));
    about["accounts"] = json!([
        { "system": "github", "host": "github.com" },
        { "system": "confluence", "host": "acme.atlassian.net" }
    ]);
    about["sharepoint_check_scope"] = json!(SCOPE);
    about
}

/// The Background of the amendment: a front door listing GitHub and Confluence and a SharePoint scope.
async fn linking_agent(fx: &Fixture, host: &str) -> Agent {
    let agent = fx.stand_in(host, "kfd").await;
    agent.about(about_with_accounts("kfd")).await;
    agent.contents(contents_json("kfd")).await;
    agent
}

async fn accounts_posts(agent: &Agent) -> Vec<Value> {
    agent
        .requests()
        .await
        .into_iter()
        .filter(|r| r.method.as_str() == "POST" && r.url.path() == "/v1/knowledge/accounts")
        .map(|r| body_of(&r))
        .collect()
}

#[tokio::test]
async fn linking_needs_no_step_from_the_person() {
    let fx = Fixture::new().await;
    fx.tokens.set_credentials(vec![
        credential(CredentialKind::Github, "github.com"),
        credential(CredentialKind::Confluence, "acme.atlassian.net"),
    ]);
    let agent = linking_agent(&fx, "kfd.example.org").await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            EchoLinked { account_id: None },
        )
        .await;

    let (view, _) = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();

    let posts = accounts_posts(&agent).await;
    assert_eq!(posts.len(), 1, "one POST /v1/knowledge/accounts");
    let systems: Vec<&str> = posts[0]["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["system"].as_str().unwrap())
        .collect();
    assert_eq!(systems, ["github", "confluence"]);
    assert_eq!(posts[0]["accounts"][1]["token"], "confluence-token");
    let post = agent
        .requests()
        .await
        .into_iter()
        .find(|r| r.url.path() == "/v1/knowledge/accounts")
        .unwrap();
    assert_eq!(
        header_of(&post, "authorization"),
        format!("Bearer {}", fx.tokens.bearer("aud-kfd"))
    );

    let details = fx.client.details(view.connection_id).await.unwrap();
    let states: Vec<(SourceSystem, AccountLinkState)> = details
        .accounts
        .iter()
        .map(|a| (a.system, a.state))
        .collect();
    assert_eq!(
        states,
        [
            (SourceSystem::Github, AccountLinkState::Linked),
            (SourceSystem::Confluence, AccountLinkState::Linked)
        ]
    );
    assert_eq!(
        accounts_posts(&agent).await.len(),
        1,
        "details does not link again"
    );
}

#[tokio::test]
async fn a_system_the_person_is_not_signed_in_to_is_not_linked() {
    let fx = Fixture::new().await;
    fx.tokens
        .set_credentials(vec![credential(CredentialKind::Github, "github.com")]);
    let agent = linking_agent(&fx, "kfd.example.org").await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            EchoLinked { account_id: None },
        )
        .await;
    let (view, _) = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();

    let posts = accounts_posts(&agent).await;
    assert_eq!(posts.len(), 1);
    let sent = posts[0]["accounts"].as_array().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["system"], "github");

    let details = fx.client.details(view.connection_id).await.unwrap();
    let confluence = details
        .accounts
        .iter()
        .find(|a| a.system == SourceSystem::Confluence)
        .expect("confluence is listed");
    assert_eq!(confluence.state, AccountLinkState::NotSignedIn);
    // The screens show their sentence for exactly this state; the crate names the system only.
    assert_eq!(confluence.system.display(), "Confluence");
}

#[tokio::test]
async fn signing_in_later_links_at_once_signing_out_unlinks() {
    let fx = Fixture::new().await;
    fx.tokens
        .set_credentials(vec![credential(CredentialKind::Github, "github.com")]);
    let agent = linking_agent(&fx, "kfd.example.org").await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            EchoLinked { account_id: None },
        )
        .await;
    agent
        .route(
            "DELETE",
            "/v1/knowledge/accounts/github",
            ResponseTemplate::new(200),
        )
        .await;
    let (view, _) = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    fx.client.inner.start_sign_in_loop();
    eventually("the sign-in listener", || async {
        fx.tokens.changes.receiver_count() > 0
    })
    .await;

    // The person signs in to Confluence in ordin.
    let confluence = credential(CredentialKind::Confluence, "acme.atlassian.net");
    let github = fx.tokens.credentials.lock().unwrap()[0].clone();
    fx.tokens
        .set_credentials(vec![github.clone(), confluence.clone()]);
    let started = Instant::now();
    fx.tokens
        .changes
        .send(ordin_identity::SignInChanged {
            credential_id: confluence.id,
            credential_kind: CredentialKind::Confluence,
            state: ordin_identity::SignInState::SignedIn,
        })
        .unwrap();
    eventually("the Confluence link", || async {
        accounts_posts(&agent).await.len() == 2
    })
    .await;
    assert!(started.elapsed() < Duration::from_secs(5), "within 5 s");
    let second = &accounts_posts(&agent).await[1];
    let sent = second["accounts"].as_array().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["system"], "confluence");

    // The person signs out of GitHub in ordin.
    fx.tokens
        .changes
        .send(ordin_identity::SignInChanged {
            credential_id: github.id,
            credential_kind: CredentialKind::Github,
            state: ordin_identity::SignInState::SignedOut,
        })
        .unwrap();
    eventually("the unlink", || async {
        agent.requests().await.iter().any(|r| {
            r.method.as_str() == "DELETE" && r.url.path() == "/v1/knowledge/accounts/github"
        })
    })
    .await;
    eventually("the row to read NotSignedIn", || async {
        fx.client
            .details(view.connection_id)
            .await
            .map(|d| {
                d.accounts.iter().any(|a| {
                    a.system == SourceSystem::Github && a.state == AccountLinkState::NotSignedIn
                })
            })
            .unwrap_or(false)
    })
    .await;
}

#[tokio::test]
async fn tokens_never_stay_on_the_desktops_tables_or_logs() {
    let capture = LogCapture::start();
    let fx = Fixture::new().await;
    fx.tokens.set_credentials(vec![
        credential(CredentialKind::Github, "github.com"),
        credential(CredentialKind::Confluence, "acme.atlassian.net"),
    ]);
    let agent = linking_agent(&fx, "kfd.example.org").await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            EchoLinked {
                account_id: Some("acct-4411"),
            },
        )
        .await;
    let (view, _) = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    fx.client.details(view.connection_id).await.unwrap();
    // An ask logs its line too.
    agent.answers(&[hit("kfd", "a.md", "a fact")]).await;
    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "a secret question"))
        .await
        .unwrap();
    let _ = stream.first().await;

    let secrets = [
        "source-token-",
        "confluence-token",
        "company-aud-kfd",
        "acct-4411",
    ];
    // Every text column of every table of the module.
    use sqlx::Row;
    for table in [
        "kq_connections",
        "kq_connection_contents",
        "kq_connection_accounts",
    ] {
        let sql = format!("SELECT * FROM {table}");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&fx.pool())
            .await
            .unwrap();
        for row in rows {
            for column in 0..row.len() {
                if let Ok(Some(text)) = row.try_get::<Option<String>, _>(column) {
                    for secret in secrets {
                        assert!(!text.contains(secret), "{table} holds {secret}");
                    }
                }
            }
        }
    }
    let entries = fx
        .journal
        .list_entries(ordin_journal::EntryFilter::default(), None)
        .await
        .unwrap();
    let journal_text = serde_json::to_string(&entries.items).unwrap();
    for secret in secrets {
        assert!(!journal_text.contains(secret));
    }
    let log = capture.text();
    assert!(
        log.contains("kq_accounts_linked"),
        "the linking line is logged: {log}"
    );
    for secret in secrets.iter().chain(&["a secret question"]) {
        assert!(!log.contains(secret), "the log holds {secret}");
    }
}

#[tokio::test]
async fn a_refused_link_shows_on_the_details_a_temporary_refusal_changes_nothing() {
    // Given the front door refuses GitHub with not_accepted.
    let fx = Fixture::new().await;
    fx.tokens.set_credentials(vec![
        credential(CredentialKind::Github, "github.com"),
        credential(CredentialKind::Confluence, "acme.atlassian.net"),
    ]);
    let agent = linking_agent(&fx, "kfd.example.org").await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            ResponseTemplate::new(200).set_body_json(json!({
                "linked": [{ "system": "confluence" }],
                "refused": [{ "system": "github", "code": "KQ_ACCOUNT_LINK_REFUSED", "reason": "not_accepted" }]
            })),
        )
        .await;
    let (view, _) = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    let details = fx.client.details(view.connection_id).await.unwrap();
    let github = details
        .accounts
        .iter()
        .find(|a| a.system == SourceSystem::Github)
        .unwrap();
    assert_eq!(github.state, AccountLinkState::Refused);
    assert_eq!(
        details.last_problem.as_deref(),
        Some("GitHub did not accept ordin's sign-in for this knowledge agent.")
    );

    // Given the front door answers try_again for Confluence.
    let fx = Fixture::new().await;
    fx.tokens.set_credentials(vec![
        credential(CredentialKind::Github, "github.com"),
        credential(CredentialKind::Confluence, "acme.atlassian.net"),
    ]);
    let agent = linking_agent(&fx, "kfd.example.org").await;
    agent
        .route(
            "POST",
            "/v1/knowledge/accounts",
            ResponseTemplate::new(200).set_body_json(json!({
                "linked": [{ "system": "github" }],
                "refused": [{ "system": "confluence", "code": "KQ_ACCOUNT_LINK_REFUSED", "reason": "try_again" }]
            })),
        )
        .await;
    let (view, _) = fx
        .client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    let details = fx.client.details(view.connection_id).await.unwrap();
    let confluence = details
        .accounts
        .iter()
        .find(|a| a.system == SourceSystem::Confluence)
        .unwrap();
    assert_eq!(confluence.state, AccountLinkState::NotSignedIn, "unchanged");
    assert_eq!(details.last_problem, None);
}

/// FakeTokens for everything but the scope token, which comes from the real Identity Service.
struct RealScopeToken {
    fake: Arc<FakeTokens>,
    real: IdentityTokens,
}

#[async_trait::async_trait]
impl TokenSource for RealScopeToken {
    async fn audience_token(
        &self,
        issuer: &str,
        client_id: &str,
        audience: &str,
    ) -> Result<AccessToken, CredError> {
        self.fake.audience_token(issuer, client_id, audience).await
    }
    async fn invalidate_audience(&self, issuer: &str, client_id: &str, audience: &str) {
        self.fake
            .invalidate_audience(issuer, client_id, audience)
            .await
    }
    async fn credentials(&self) -> Result<Vec<LinkCredential>, CredError> {
        self.fake.credentials().await
    }
    async fn token_by_id(&self, id: CredentialId) -> Result<AccessToken, CredError> {
        self.fake.token_by_id(id).await
    }
    async fn confluence_read_me(&self) -> Result<AccessToken, CredError> {
        self.fake.confluence_read_me().await
    }
    async fn scope_token(
        &self,
        kind: CredentialKind,
        scope: &str,
    ) -> Result<AccessToken, CredError> {
        self.real.scope_token(kind, scope).await
    }
    fn subscribe(&self) -> broadcast::Receiver<SignInChanged> {
        self.fake.subscribe()
    }
}

/// A real Identity Service with a signed-in Microsoft credential, its tokens from `token_endpoint`.
async fn real_identity(dir: &TempDir, token_endpoint: &str) -> Arc<IdentityService> {
    let key_file = dir.0.join("secrets.key");
    std::fs::write(&key_file, BASE64_KEY).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(dir.0.join("identity.db"))
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await.unwrap();
    for (_, _, sql) in ordin_identity::SQLITE_MIGRATIONS {
        sqlx::raw_sql(*sql).execute(&pool).await.unwrap();
    }
    let identity = IdentityService::new(
        IdentityConfig {
            program: Program::PersonalServer,
            data_root: dir.0.clone(),
            keychain_service: "io.gatblau.ordin.test".to_owned(),
            secret_backend: SecretBackendConfig::EncryptedFile {
                key_source: KeySource::KeyFile(key_file),
            },
            github_app_client_id: None,
            http: reqwest::Client::new(),
            browser: None,
        },
        Some(pool),
    )
    .await
    .expect("identity");
    identity
        .seed_microsoft_for_tests("ms-refresh", "ordin-desktop-app", token_endpoint, &[])
        .await
        .expect("seed");
    Arc::new(identity)
}

// Plan 01 Phase 6A: the delegation of PD-B through the real token source (FU-024).
#[tokio::test]
async fn a_delegation_goes_out_through_the_real_token_source() {
    let provider = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("offline_access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "real-assertion", "expires_in": 3600, "token_type": "Bearer"
        })))
        .expect(1)
        .mount(&provider)
        .await;
    let dir = TempDir::new();
    let identity = real_identity(&dir, &format!("{}/token", provider.uri())).await;
    let fx = Fixture::with_token_source(quick_timings(), |fake| {
        Arc::new(RealScopeToken {
            fake,
            real: IdentityTokens(identity),
        })
    })
    .await;
    let agent = linking_agent(&fx, "kfd.example.org").await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    agent.answers(&[hit("kfd", "a.md", "fact one")]).await;

    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "question"))
        .await
        .unwrap();
    assert_eq!(stream.first().await.hits.len(), 1);

    let sent = body_of(&agent.query_requests().await[0]);
    assert_eq!(
        sent["delegation"],
        json!({ "sharepoint_assertion": "real-assertion" })
    );
    let asked = provider.received_requests().await.unwrap();
    let form = String::from_utf8_lossy(&asked[0].body);
    assert!(form.contains("scope=api%3A%2F%2F7d1c0000%2Fknowledge.check+offline_access"));
    provider.verify().await;
}

#[tokio::test]
async fn delegation_travels_only_to_its_own_front_door() {
    let fx = Fixture::new().await;
    let first = linking_agent(&fx, "kfd.example.org").await;
    first
        .route(
            "POST",
            "/v1/knowledge/accounts",
            EchoLinked { account_id: None },
        )
        .await;
    let second = fx.agent("second.example.org", "second").await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    fx.client
        .add_connection(fx.ws, "https://second.example.org")
        .await
        .unwrap();
    first.answers(&[hit("kfd", "a.md", "fact one")]).await;
    second.answers(&[hit("second", "b.md", "fact two")]).await;

    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "question"))
        .await
        .unwrap();
    let batch = stream.first().await;
    assert_eq!(batch.hits.len(), 2);

    let with = body_of(&first.query_requests().await[0]);
    assert_eq!(
        with["delegation"],
        json!({ "sharepoint_assertion": format!("assertion-for-{SCOPE}") })
    );
    let without = body_of(&second.query_requests().await[0]);
    assert!(without.get("delegation").is_none());
    assert_eq!(*fx.tokens.scope_calls.lock().unwrap(), [SCOPE]);
    assert_eq!(fx.client.microsoft_check_scopes().await, [SCOPE]);
}

#[tokio::test]
async fn unchecked_sources_reach_the_memory_service() {
    let fx = Fixture::new().await;
    let agent = fx.agent("kfd.example.org", "kfd").await;
    fx.client
        .add_connection(fx.ws, "https://kfd.example.org")
        .await
        .unwrap();
    let mut result = result_event(&[hit("kfd", "a.md", "a fact")]);
    result["unchecked_sources"] = json!(true);
    agent.query(sse_template(sse(&[("result", result)]))).await;

    let mut stream = fx
        .client
        .ask(fx.ask(AskTarget::Workspace(fx.ws), "question"))
        .await
        .unwrap();
    let first = stream.first().await;
    assert!(first.unchecked_sources);
    assert_eq!(first.hits.len(), 1);
}

#[tokio::test]
async fn a_front_doors_asker_reaches_the_proxies_pending_hits_stay_apart() {
    let (fx, agents) = proxies(&["p1"]).await;
    let p1 = &agents[1];
    let pending_hit = hit("p1", "restricted.md", "a fact the front door must check");
    let mut result = result_event(&[hit("p1", "open.md", "an open fact")]);
    result["pending"] = json!([{ "hit": pending_hit, "unit": { "system": "sharepoint", "unit": "drive-1/item-2" } }]);
    p1.query(sse_template(sse(&[("result", result)]))).await;

    let mut ask = fx.ask(
        AskTarget::Proxies(vec![ProxyAsk {
            primary: "p1".into(),
            backup: None,
        }]),
        "question",
    );
    ask.access_group = Some("procurement".to_owned());
    ask.asker = Some(AskerStatement {
        subject: "00u1".to_owned(),
        accounts: [(SourceSystem::Github, "4411".to_owned())]
            .into_iter()
            .collect(),
        issued_at: ts(),
    });
    let mut stream = fx.client.ask(ask).await.unwrap();
    let first = stream.first().await;

    let body = body_of(&p1.query_requests().await[0]);
    assert_eq!(body["asker"]["subject"], "00u1");
    assert_eq!(body["asker"]["accounts"], json!({ "github": "4411" }));
    assert_eq!(body["access_group"], "procurement");
    assert!(body.get("delegation").is_none());
    assert_eq!(first.hits.len(), 1);
    assert_eq!(first.hits[0].path, "open.md");
    assert_eq!(first.pending.len(), 1);
    assert_eq!(first.pending[0].0, "p1");
    assert_eq!(first.pending[0].1.hit.path, "restricted.md");
    assert_eq!(
        first.pending[0].1.unit,
        AccessUnit {
            system: SourceSystem::Sharepoint,
            unit: "drive-1/item-2".to_owned(),
        }
    );
    // The proxy was asked with a service token, minted for its audience.
    let query = &p1.query_requests().await[0];
    assert_eq!(header_of(query, "authorization"), "Bearer svc-token");
}
