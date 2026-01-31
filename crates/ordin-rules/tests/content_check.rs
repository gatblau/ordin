//! The seven content-check scenarios of the ADR-107 amendment (RulesEngine.md *Acceptance Criteria*,
//! added scenarios), against a `wiremock` checker stand-in on loopback that answers allow, block or
//! never (plan 01, Phase 4, Step 27). The checker is reached over real HTTP through `HttpChecker`.

mod support;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_rules::{ConfiguredControls, ContentCheckOutcome, ControlSource, Point, ProgramKind};
use ordin_types::domain::agent_profile::{CheckPoint, ContentCheck, OnBlock, OnUnreachable};
use ordin_types::domain::agents::Authority;
use ordin_types::{AgentInstanceId, RunId};
use serde_json::json;
use support::{fixture, rule_set, Fixture, HttpChecker};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const QUESTION: &str = "How do I reset the VPN on my laptop?";

fn screen(points: Vec<CheckPoint>) -> ContentCheck {
    ContentCheck {
        name: "screen".to_owned(),
        connection: "moderation".to_owned(),
        operation: "check".to_owned(),
        points,
        on_block: OnBlock::RetryWithoutContext,
        on_unreachable: OnUnreachable::Block,
        categories: Vec::new(),
        timeout_seconds: 1,
        max_chars: 4_000,
    }
}

/// An agent-profile engine whose checker is the stand-in, with `check` loaded.
async fn with_check(server: &MockServer, check: ContentCheck) -> (Fixture, AgentInstanceId) {
    let f = fixture(ProgramKind::AgentSingle).await;
    *f.host.checker.lock().unwrap() = Some(Arc::new(HttpChecker {
        base: server.uri(),
        client: reqwest::Client::new(),
    }));
    let mut set = rule_set(Authority::DocumentOnly, &f.folder("helpdesk"));
    set.configured = Some(ConfiguredControls {
        content_checks: vec![check],
        ..ConfiguredControls::default()
    });
    let agent = set.agent_instance_id;
    f.engine.load_rule_set(set).expect("the set loads");
    (f, agent)
}

async fn answering(template: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/moderation/check"))
        .respond_with(template)
        .mount(&server)
        .await;
    server
}

fn block(category: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"verdict": "block", "category": category}))
}

async fn bodies(server: &MockServer) -> Vec<serde_json::Value> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).expect("a JSON body"))
        .collect()
}

/// The log lines written while `body` runs (on this test's single thread).
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

fn capture() -> (Captured, tracing::subscriber::DefaultGuard) {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    (captured, tracing::subscriber::set_default(subscriber))
}

#[tokio::test]
async fn the_input_check_blocks() {
    let server = answering(block("prompt-attack")).await;
    let (f, agent) = with_check(&server, screen(vec![CheckPoint::Input])).await;
    let run = RunId::new();
    let outcome = f
        .engine
        .check_content(agent, run, CheckPoint::Input, QUESTION)
        .await;
    assert_eq!(
        outcome,
        ContentCheckOutcome::Blocked {
            control: "screen".to_owned(),
            category: Some("prompt-attack".to_owned()),
            on_block: OnBlock::RetryWithoutContext,
            unreachable: false,
        }
    );
    assert_eq!(
        bodies(&server).await,
        vec![json!({"point": "input", "run_id": run.to_string(), "text": QUESTION})]
    );
    // The journal holds the verdict and never the text.
    let data: Vec<String> =
        sqlx::query_scalar("SELECT data FROM journal_entries WHERE kind = 'rules_content_check'")
            .fetch_all(f.pool())
            .await
            .unwrap();
    assert_eq!(data.len(), 1);
    assert!(data[0].contains("\"verdict\":\"block\""));
    assert!(!data[0].contains("VPN"));
}

#[tokio::test]
async fn an_unreachable_checker_blocks_by_default() {
    // "never": the stand-in answers long after the control's one-second timeout.
    let server = answering(
        ResponseTemplate::new(200)
            .set_body_json(json!({"verdict": "allow"}))
            .set_delay(Duration::from_secs(5)),
    )
    .await;
    let (f, agent) = with_check(&server, screen(vec![CheckPoint::Input])).await;
    let (logs, _guard) = capture();
    let outcome = f
        .engine
        .check_content(agent, RunId::new(), CheckPoint::Input, QUESTION)
        .await;
    assert_eq!(
        outcome,
        ContentCheckOutcome::Blocked {
            control: "screen".to_owned(),
            category: None,
            on_block: OnBlock::End,
            unreachable: true,
        },
        "on_block is End even though the control says retry-without-context"
    );
    let text = logs.text();
    assert!(text.contains("RULE_CHECKER_UNREACHABLE"), "{text}");
    assert!(text.contains("timeout"), "{text}");
    assert!(!text.contains("VPN"), "the text is never logged");
}

#[tokio::test]
async fn an_unreachable_checker_with_on_unreachable_allow_lets_the_text_through() {
    let server = answering(ResponseTemplate::new(503)).await;
    let mut check = screen(vec![CheckPoint::Input]);
    check.on_unreachable = OnUnreachable::Allow;
    let (f, agent) = with_check(&server, check).await;
    let (logs, _guard) = capture();
    let outcome = f
        .engine
        .check_content(agent, RunId::new(), CheckPoint::Input, QUESTION)
        .await;
    assert_eq!(outcome, ContentCheckOutcome::Passed);
    let text = logs.text();
    assert!(text.contains("RULE_CHECKER_UNREACHABLE"), "{text}");
    assert!(text.contains("status"), "{text}");
}

#[tokio::test]
async fn a_block_outside_the_listed_categories_counts_as_allow() {
    let server = answering(block("profanity")).await;
    let mut check = screen(vec![CheckPoint::Input]);
    check.categories = vec!["denied-topic".to_owned()];
    let (f, agent) = with_check(&server, check).await;
    let outcome = f
        .engine
        .check_content(agent, RunId::new(), CheckPoint::Input, QUESTION)
        .await;
    assert_eq!(outcome, ContentCheckOutcome::Passed);
}

#[tokio::test]
async fn no_check_covers_the_point() {
    let server = answering(block("prompt-attack")).await;
    let (f, agent) = with_check(&server, screen(vec![CheckPoint::Output])).await;
    let outcome = f
        .engine
        .check_content(agent, RunId::new(), CheckPoint::Input, QUESTION)
        .await;
    assert_eq!(outcome, ContentCheckOutcome::Passed);
    assert!(
        bodies(&server).await.is_empty(),
        "the checker is not called"
    );
}

#[tokio::test]
async fn the_text_is_cut_to_max_chars() {
    let server =
        answering(ResponseTemplate::new(200).set_body_json(json!({"verdict": "allow"}))).await;
    let mut check = screen(vec![CheckPoint::Input]);
    check.max_chars = 10;
    let (f, agent) = with_check(&server, check).await;
    let question = "abcdéfghijklmnopqrstuvwxy";
    assert_eq!(question.chars().count(), 25);
    let outcome = f
        .engine
        .check_content(agent, RunId::new(), CheckPoint::Input, question)
        .await;
    assert_eq!(outcome, ContentCheckOutcome::Passed);
    let sent = bodies(&server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["text"], "abcdéfghij");
}

#[tokio::test]
async fn the_start_up_report_names_the_checker() {
    let f = fixture(ProgramKind::AgentSingle).await;
    let mut set = rule_set(Authority::DocumentOnly, &f.folder("helpdesk"));
    set.configured = Some(ConfiguredControls {
        content_checks: vec![screen(vec![CheckPoint::Input, CheckPoint::Output])],
        ..ConfiguredControls::default()
    });
    let lines = f.engine.load_rule_set(set).expect("the set loads");
    let checks: Vec<_> = lines.iter().filter(|l| l.name == "screen").collect();
    assert_eq!(checks.len(), 2, "one line per point");
    for line in &checks {
        assert_eq!(line.source, ControlSource::Configuration);
        assert!(line.enforced);
        assert!(line.detail.contains("moderation/check"), "{}", line.detail);
    }
    let points: Vec<Point> = checks.iter().map(|l| l.point).collect();
    assert!(points.contains(&Point::Start));
    assert!(points.contains(&Point::End));
}
