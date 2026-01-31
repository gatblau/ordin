//! Observability (ApprovalService.md; Logging.md R-2): the log events of the service, their
//! fields, and what they never hold. The only test in its own binary: `tracing` caches per call
//! site whether anything listens, so one process-wide subscriber is installed before anything runs
//! and no other test shares the process (plan FU-017).

mod support;

use std::sync::{Arc, Mutex};

use ordin_approval::PressOrigin;
use ordin_types::wire::approval::ApprovalAsker;
use ordin_types::RunId;
use support::*;
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn lines(buffer: &Buffer) -> Vec<serde_json::Value> {
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    text.lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect()
}

fn named<'a>(all: &'a [serde_json::Value], event: &str) -> Vec<&'a serde_json::Value> {
    all.iter().filter(|line| line["event"] == event).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn each_event_is_logged_with_its_fields_and_never_with_card_text() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    tracing::subscriber::set_global_default(subscriber).expect("install the subscriber");

    // Desktop profile.
    let f = fixture().await;
    let run_id = RunId::new();
    let ws = workspace();
    let card = f
        .service
        .request(request_for(ws, ApprovalAsker::Run { run_id }))
        .await
        .unwrap();
    f.service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();
    let withdrawn = f
        .service
        .request(request_for(ws, ApprovalAsker::Person))
        .await
        .unwrap();
    f.service.withdraw(withdrawn.approval_id).await.unwrap();
    let ended = RunId::new();
    let expiring = f
        .service
        .request(request_for(ws, ApprovalAsker::Run { run_id: ended }))
        .await
        .unwrap();
    f.service.expire_for_run(ended).await.unwrap();
    let _pending = f.service.request(request()).await.unwrap();
    let _restarted = f.restart().await;
    f.service.event_sink().lagged(42).await;

    // Agent profile: the approver's subject is logged as `approver`.
    let agent = fixture_with(Some(policy(&["u-456"], &[], false))).await;
    let agent_card = agent.service.request(request()).await.unwrap();
    agent
        .service
        .decide(approve(&agent_card), approver("u-456", &[]))
        .await
        .unwrap();

    // A failing emit is logged under its own event and code.
    sqlx::query("DROP TABLE journal_events")
        .execute(f.pool())
        .await
        .unwrap();
    f.service.request(request()).await.unwrap();

    let all = lines(&buffer);
    let ours: Vec<&serde_json::Value> = all
        .iter()
        .filter(|line| {
            line["event"]
                .as_str()
                .is_some_and(|event| event.starts_with("approval_"))
        })
        .collect();
    // No field was dropped by the formatter (Logging.md R-2).
    for line in &all {
        assert!(line.get("dropped_fields").is_none(), "{line}");
    }

    let requested = named(&all, "approval_requested");
    assert_eq!(requested.len(), 6);
    assert_eq!(requested[0]["approval_id"], card.approval_id.to_string());
    assert_eq!(requested[0]["workspace_id"], ws.to_string());
    assert_eq!(requested[0]["asked_by_kind"], "run");
    assert_eq!(requested[0]["run_id"], run_id.to_string());
    assert_eq!(requested[1]["asked_by_kind"], "person");

    let decided = named(&all, "approval_decided");
    assert_eq!(decided.len(), 2);
    assert_eq!(decided[0]["state"], "approved");
    assert_eq!(decided[0]["origin_kind"], "local_screen");
    assert!(
        decided[0].get("approver").is_none(),
        "desktop profiles log no approver"
    );
    assert_eq!(decided[1]["origin_kind"], "approver");
    assert_eq!(decided[1]["approver"], "u-456");

    let withdrawn_lines = named(&all, "approval_withdrawn");
    assert_eq!(withdrawn_lines.len(), 1);
    assert_eq!(
        withdrawn_lines[0]["approval_id"],
        withdrawn.approval_id.to_string()
    );

    let expired = named(&all, "approval_expired");
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0]["approval_id"], expiring.approval_id.to_string());
    assert_eq!(expired[0]["run_id"], ended.to_string());

    // Three starts: the first fixture, its restart (which found the one approval left pending) and
    // the agent-profile fixture.
    let recovered = named(&all, "approval_recovered");
    assert_eq!(recovered.len(), 3);
    assert_eq!(recovered[0]["count"], 0);
    assert_eq!(recovered[1]["count"], 1);

    let lagged = named(&all, "approval_sink_lagged");
    assert_eq!(lagged.len(), 1);
    assert_eq!(lagged[0]["next_seq"], 42);

    let failed = named(&all, "approval_emit_failed");
    assert!(!failed.is_empty());
    assert_eq!(failed[0]["code"], "APPR_EMIT_FAILED");
    assert_eq!(failed[0]["error_code"], "JRNL_DATASTORE_UNAVAILABLE");

    // Never logged: card text, the asker's identity, a card reference.
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    for secret in [
        "Push 3 commits",
        "supplier-portal",
        "everyone who can open",
        &ref_text(&card.buttons.approve),
        &ref_text(&card.buttons.reject),
        COMPUTER,
    ] {
        assert!(!text.contains(secret), "the log holds {secret}");
    }
    assert!(!ours.is_empty());
}
