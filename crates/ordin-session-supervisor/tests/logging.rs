//! The scenarios about log lines: the run summary of each stretch (H.2, ADR-107), the blocked text
//! that is never logged (J.7.2), and `sup_run_starting_slow` (ADR-111). This binary owns its
//! process's log subscriber, which keeps every event; each test reads only its run's lines.

mod support;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use ordin_journal::NewExternalAction;
use ordin_runtime_adapter::{PauseReason, RunEndReason, RunEvent};
use ordin_session_supervisor::{ContentCheck, ProgramKind};
use ordin_types::domain::agent_profile::{RunRecordState, RunRequest};
use ordin_types::domain::runs::{ExternalActionKind, RunState, TaskArgument};
use ordin_types::{ApprovalId, ExternalActionKey, RunId};
use support::*;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

/// One event: its message and its fields as text.
#[derive(Debug, Clone)]
struct Line {
    message: String,
    fields: HashMap<String, String>,
    text: String,
}

fn lines() -> &'static Mutex<Vec<Line>> {
    static LINES: OnceLock<Mutex<Vec<Line>>> = OnceLock::new();
    LINES.get_or_init(|| Mutex::new(vec![]))
}

#[derive(Default)]
struct Fields(HashMap<String, String>);

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

struct Keep;

impl<S: tracing::Subscriber> Layer<S> for Keep {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let message = fields.0.remove("message").unwrap_or_default();
        let text = format!("{message} {:?}", fields.0);
        lines().lock().unwrap().push(Line {
            message,
            fields: fields.0,
            text,
        });
    }
}

fn install() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry().with(Keep));
    });
}

/// The lines of `message` whose `run_id` is `run`.
fn of(message: &str, run: RunId) -> Vec<Line> {
    let run = run.to_string();
    lines()
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.message == message && l.fields.get("run_id") == Some(&run))
        .cloned()
        .collect()
}

fn json(line: &Line, field: &str) -> serde_json::Value {
    serde_json::from_str(line.fields.get(field).map(String::as_str).unwrap_or("null")).unwrap()
}

async fn wait_summaries(run: RunId, n: usize) -> Vec<Line> {
    for _ in 0..500 {
        let found = of("run_summary", run);
        if found.len() >= n {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{n} run summaries never written for {run}");
}

#[tokio::test]
async fn agent_profile_the_run_summary_names_the_runs_external_actions() {
    install();
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    let record = h
        .enqueue(
            chat("U7"),
            message("Post the totals"),
            serde_json::json!({}),
        )
        .await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    let key = ExternalActionKey::new();
    h.journal
        .begin_external_action(NewExternalAction {
            action_key: key,
            run_id: Some(run),
            kind: ExternalActionKind::SlackPost,
            destination: record.thread_id.clone(),
        })
        .await
        .unwrap();
    h.journal.confirm_external_action(key).await.unwrap();
    h.local.end(run, RunEndReason::Completed, Some("Posted."));
    let view = h.wait_record(&record.run_id, RunRecordState::Ended).await;
    let summary = wait_summaries(run, 1).await.pop().unwrap();
    assert_eq!(
        json(&summary, "actions_attached"),
        serde_json::json!(["slack_post"])
    );
    assert_eq!(
        json(&summary, "controls_decided"),
        serde_json::json!([["slack_post", "confirmed"]])
    );
    assert_eq!(summary.fields["state"], "finished");
    assert_eq!(summary.fields["delivery"], record.thread_id);
    let outcome = view.outcome.unwrap();
    assert_eq!(outcome.actions_attached, ["slack_post"]);
    assert_eq!(outcome.delivery, Some(ordin_run_queue::Delivery::Thread));
}

#[tokio::test]
async fn agent_profile_one_run_summary_per_stretch() {
    install();
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    h.journal.subscribe(h.sup.event_sink());
    let record = h
        .enqueue(
            chat("U-asker-42"),
            RunRequest::Command {
                command: "close-ticket".into(),
                arguments: vec![TaskArgument {
                    name: "ticket".into(),
                    value: "secret-argument-9".into(),
                }],
            },
            serde_json::json!({}),
        )
        .await;
    *h.host.expansion.lock().unwrap() = Ok("secret-expanded-message".into());
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    h.sup
        .save_session_record(run, &TestAdapter::session_of(run), b"rec".to_vec())
        .await
        .unwrap();
    let approval_id = ApprovalId::new();
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Approval { approval_id },
        },
    );
    h.wait_record(&record.run_id, RecordState::Paused).await;
    h.journal
        .append_event(
            "approval_decided",
            serde_json::json!({ "approval_id": approval_id, "state": "approved", "decided_on": "screen" }),
        )
        .await
        .unwrap();
    Node::wait_until("resumed", || !h.local.resumes.lock().unwrap().is_empty()).await;
    h.local.push(run, h.local.started(run));
    h.local
        .end(run, RunEndReason::Completed, Some("secret-final-message"));
    h.wait_record(&record.run_id, RecordState::Ended).await;
    let summaries = wait_summaries(run, 2).await;
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].fields["state"], "paused");
    assert_eq!(summaries[0].fields["resumed"], "false");
    assert_eq!(summaries[1].fields["state"], "finished");
    assert_eq!(summaries[1].fields["resumed"], "true");
    assert_eq!(summaries[1].fields["command"], "close-ticket");
    assert_eq!(summaries[1].fields["started_by"], "chat");
    for summary in &summaries {
        for secret in [
            "U-asker-42",
            "secret-argument-9",
            "secret-expanded-message",
            "secret-final-message",
        ] {
            assert!(!summary.text.contains(secret), "{secret} in a run summary");
        }
    }
    let stored = h.record(&record.run_id).await.record.stretch_summaries;
    assert_eq!(stored.len(), 2);
}

type RecordState = RunRecordState;

#[tokio::test]
async fn agent_profile_a_blocked_answer_with_on_block_end_is_never_delivered() {
    install();
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    {
        let mut checks = h.host.checks.lock().unwrap();
        checks.push_back(ContentCheck::Passed);
        checks.push_back(ContentCheck::Blocked {
            retry_without_context: false,
        });
    }
    let record = h
        .enqueue(chat("U1"), message("Question"), serde_json::json!({}))
        .await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    h.local
        .end(run, RunEndReason::Completed, Some("BLOCKED-TEXT-7731"));
    let view = h.wait_record(&record.run_id, RecordState::Ended).await;
    let outcome = view.outcome.unwrap();
    assert!(outcome.safe_reply);
    assert_eq!(outcome.reply.as_deref(), Some(SAFE_REPLY));
    assert_eq!(h.local.start_count(), 1);
    wait_summaries(run, 1).await;
    assert!(lines()
        .lock()
        .unwrap()
        .iter()
        .all(|l| !l.text.contains("BLOCKED-TEXT-7731")));
    for kind in ["run_result", "run_code"] {
        for entry in h.entries(run, kind).await {
            assert!(!entry.to_string().contains("BLOCKED-TEXT-7731"));
        }
    }
    let done = h.sup.get(run).await.unwrap().run;
    assert_eq!(done.end_reason, None);
}

#[tokio::test]
async fn agent_profile_the_run_summary_lists_each_context_step() {
    install();
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    let context = serde_json::json!({ "context": [
        { "kind": "piece", "piece": "transcript", "label": "thread", "media_type": "text/plain",
          "content": "secret transcript", "size_bytes": 1200, "cut": false, "step": 1, "items": 12 },
        { "kind": "left_out", "piece": "knowledge", "label": "faq", "reason": "empty", "step": 2 }
    ]});
    let record = h.enqueue(chat("U1"), message("Question"), context).await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    h.local.end(run, RunEndReason::Completed, Some("Answer"));
    h.wait_record(&record.run_id, RecordState::Ended).await;
    let summary = wait_summaries(run, 1).await.pop().unwrap();
    let steps = json(&summary, "context");
    assert_eq!(steps.as_array().unwrap().len(), 2);
    assert_eq!(steps[0]["kind"], "transcript");
    assert_eq!(steps[0]["outcome"], "piece");
    assert_eq!(steps[0]["items"], 12);
    assert_eq!(steps[1]["kind"], "knowledge-lookup");
    assert_eq!(steps[1]["outcome"], "empty");
    assert_eq!(steps[1]["items"], 0);
    assert!(!summary.text.contains("secret transcript"));
}

#[tokio::test]
async fn edge_a_slow_host_does_not_hold_the_run() {
    install();
    let h = Harness::desktop().await;
    *h.host.starting_delay.lock().unwrap() = Duration::from_secs(5);
    let began = std::time::Instant::now();
    let run = h.submit("compare-quotes").await;
    Node::wait_starts(&h.local, 1).await;
    let waited = began.elapsed();
    assert!(
        waited >= Duration::from_secs(2) && waited < Duration::from_secs(4),
        "{waited:?}"
    );
    assert_eq!(of("sup_run_starting_slow", run).len(), 1);
}

#[tokio::test]
async fn ended_runs_are_logged_without_their_text() {
    install();
    let h = Harness::desktop().await;
    let mut t = task("compare-quotes");
    t.arguments[0].value = "secret-task-argument".into();
    let run = h
        .sup
        .submit_task(t, ordin_usage_planner::WorkKind::Documents, false, None)
        .await
        .unwrap();
    h.wait_state(run, RunState::Running).await;
    h.local
        .end(run, RunEndReason::Completed, Some("secret-final"));
    h.wait_state(run, RunState::Completed).await;
    Node::wait_until("ended logged", || !of("sup_run_ended", run).is_empty()).await;
    assert_eq!(of("sup_run_queued", run).len(), 1);
    assert_eq!(of("sup_run_started", run).len(), 1);
    let run_text = run.to_string();
    for line in lines()
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.text.contains(&run_text))
    {
        assert!(!line.text.contains("secret-task-argument"));
        assert!(!line.text.contains("secret-final"));
    }
}

// Scenario: Error path — a refused start keeps its code (SA-SUP-END-1). The command is the one
// Ordin passed in the field: a library use case's title, which no harness command name can hold.
// spec: ordin-desktop/components/SessionSupervisor#SA-SUP-END-1
#[tokio::test]
async fn error_a_refused_start_keeps_its_code() {
    install();
    let h = Harness::desktop().await;
    let run = h.submit("Summarise documents").await;
    let failed = h.wait_state(run, RunState::Failed).await;
    assert_eq!(
        failed.end_reason.as_deref(),
        Some("couldn't start the agent")
    );
    Node::wait_until("ended logged", || !of("sup_run_ended", run).is_empty()).await;
    let ended = of("sup_run_ended", run);
    assert_eq!(ended.len(), 1);
    assert_eq!(
        ended[0].fields.get("state").map(String::as_str),
        Some("failed")
    );
    assert_eq!(
        ended[0].fields.get("code").map(String::as_str),
        Some("RA_COMMAND_UNKNOWN")
    );
    assert_eq!(h.codes(run).await, ["RA_COMMAND_UNKNOWN"]);
    assert_eq!(h.local.start_count(), 0);
}

// SA-SUP-END-1: an ending with no code logs no code field.
#[tokio::test]
async fn edge_an_ending_without_a_code_logs_none() {
    install();
    let h = Harness::desktop().await;
    let run = h.submit("compare-quotes").await;
    h.wait_state(run, RunState::Running).await;
    h.local.end(run, RunEndReason::Completed, None);
    h.wait_state(run, RunState::Completed).await;
    Node::wait_until("ended logged", || !of("sup_run_ended", run).is_empty()).await;
    assert!(!of("sup_run_ended", run)[0].fields.contains_key("code"));
}
