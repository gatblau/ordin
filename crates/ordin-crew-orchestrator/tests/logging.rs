//! The log events of CrewOrchestrator.md *Observability* (target `ordin_crew_orchestrator`): the
//! events a crew writes, their identifiers, and the texts that are never logged. This binary owns
//! its process's log subscriber; each test reads only its own crew run's lines.

mod support;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ordin_types::domain::runs::{Condition, IfBlock, NamedPath, WorkflowStep};
use support::*;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

#[derive(Debug, Clone)]
struct Line {
    target: String,
    message: String,
    fields: HashMap<String, String>,
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
        lines().lock().unwrap().push(Line {
            target: event.metadata().target().to_owned(),
            message,
            fields: fields.0,
        });
    }
}

fn install() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let subscriber = tracing_subscriber::registry().with(Keep);
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

fn of(id: ordin_types::CrewRunId) -> Vec<Line> {
    lines()
        .lock()
        .unwrap()
        .iter()
        .filter(|l| {
            l.fields
                .get("crew_run_id")
                .is_some_and(|v| *v == id.to_string())
        })
        .cloned()
        .collect()
}

fn named(lines: &[Line], event: &str) -> Vec<Line> {
    lines
        .iter()
        .filter(|l| l.message == event)
        .cloned()
        .collect()
}

#[tokio::test]
async fn a_coordinated_crew_writes_its_events_with_identifiers_only() {
    install();
    let h = Harness::new().await;
    let a = agent();
    h.local
        .behave(|_, _| done("The secret summary of the quotes."));
    let coord = TestCoordinator::scripted(vec![
        vec![
            delegate(a, "Compare the confidential quotes"),
            delegate(agent(), "Outsider"),
        ],
        vec![finish()],
    ]);
    let id = h.start(h.charter(&[("analyst", a)]), coord.clone()).await;
    h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    // The state is set before the end line is logged, so wait for the line, as the Approval tests
    // wait for their event (FU-150).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while named(&of(id), "crew_ended").is_empty() && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let mine = of(id);
    assert!(mine.iter().all(|l| l.target == "ordin_crew_orchestrator"));
    let started = named(&mine, "crew_started");
    assert_eq!(started[0].fields["mode"], "coordinated");
    assert_eq!(started[0].fields["members"], "1");
    assert_eq!(named(&mine, "crew_round_opened").len(), 2);
    let dispatched = named(&mine, "crew_step_dispatched");
    assert_eq!(dispatched.len(), 1);
    assert!(
        dispatched[0].fields.contains_key("run_id") && dispatched[0].fields.contains_key("member")
    );
    let guardrail = named(&mine, "crew_guardrail");
    assert_eq!(guardrail[0].fields["code"], "CREW_GUARDRAIL_REFUSED");
    let ended = named(&mine, "crew_ended");
    assert_eq!(ended[0].fields["state"], "finished");
    assert_eq!(ended[0].fields["rounds"], "2");
    // Never logged: briefs, member summaries, questions and answers.
    for line in &mine {
        let text = format!("{} {:?}", line.message, line.fields);
        assert!(!text.contains("confidential"), "{text}");
        assert!(!text.contains("secret summary"), "{text}");
    }
}

#[tokio::test]
async fn a_saved_workflow_writes_its_path_and_take_over_events() {
    install();
    let h = Harness::new().await;
    let a = agent();
    let iff = WorkflowStep::If(IfBlock {
        position: "2".into(),
        condition: Condition::Ordin {
            words: "Did a private rate rise?".into(),
        },
        paths: vec![
            NamedPath {
                name: "Yes".into(),
                steps: vec![],
            },
            NamedPath {
                name: "No".into(),
                steps: vec![],
            },
        ],
    });
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![
            agent_step("1", a, "Compare"),
            iff,
            agent_step("3", a, "Write"),
        ],
    );
    h.local.behave(|req, _| {
        let text = match &req.input {
            ordin_runtime_adapter::StartInput::FreeText { text } => text.clone(),
            _ => String::new(),
        };
        if text.contains("Write") {
            fails()
        } else {
            done("Fine.")
        }
    });
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    let id = h.start(c, coord.clone()).await;
    h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    let mine = of(id);
    assert_eq!(
        named(&mine, "crew_started")[0].fields["mode"],
        "saved_steps"
    );
    let path = named(&mine, "crew_path_taken");
    assert_eq!(path[0].fields["position"], "2");
    assert_eq!(path[0].fields["decided_by"], "ordin");
    let taken = named(&mine, "crew_taken_over");
    assert_eq!(taken[0].fields["position"], "3");
    assert_eq!(taken[0].fields["reason"], "step_failed_twice");
    for line in &mine {
        assert!(!format!("{:?}", line.fields).contains("private rate"));
    }
}

#[tokio::test]
async fn recovery_writes_the_count() {
    install();
    let h = Harness::new().await;
    let a = agent();
    let stuck = TestCoordinator::new();
    stuck.stall.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = h.start(h.charter(&[("analyst", a)]), stuck.clone()).await;
    for _ in 0..500 {
        if stuck.count("round") == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let before = lines()
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.message == "crew_recovered")
        .count();
    let recovered = h.orch.recover().await.unwrap();
    assert!(recovered >= 1);
    let after = lines()
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.message == "crew_recovered")
        .count();
    assert_eq!(after, before + 1);
}
