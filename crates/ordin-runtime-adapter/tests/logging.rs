//! The violation log line (Observability; Logging.md R-2, R-7).
//!
//! The only test in its own binary: `tracing` caches per call site whether anything listens, and
//! another test thread hitting the guard's call site with no subscriber could leave this test's
//! scoped subscriber unasked. Alone in the process, no other thread logs.

use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use futures::stream::{self, StreamExt};
use ordin_runtime_adapter::*;
use ordin_types::RunId;
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

#[test]
fn each_violation_is_logged_once_with_run_adapter_and_code_only() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    let secret_question = "Shall I send the Kestrel quote to the board?";
    let events = vec![
        RunEvent::Started {
            runtime_session_id: Some("session-1".into()),
            remote_thread_id: None,
        },
        RunEvent::Paused {
            reason: PauseReason::Person {
                question: format!("{secret_question}{}", "x".repeat(500)),
            },
        },
    ];
    let run_id = RunId::parse("0192a1c2-3344-7556-8677-8899aabbccdd").unwrap();
    let items: Vec<Guarded> = tracing::subscriber::with_default(subscriber, || {
        block_on(
            guard_events(
                AdapterKind::ClaudeLocal,
                run_id,
                stream::iter(events).boxed(),
            )
            .collect(),
        )
    });
    let codes: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            Guarded::Violation(e) => Some(e.code),
            Guarded::Event(_) => None,
        })
        .collect();
    assert_eq!(codes, ["RA_EVENT_INVALID", "RA_EVENT_ORDER"]);
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "one line per violation");
    for (line, code) in lines.iter().zip(["RA_EVENT_INVALID", "RA_EVENT_ORDER"]) {
        assert_eq!(line["event"], "ra.event_violation");
        assert_eq!(line["level"], "WARNING");
        assert_eq!(line["run_id"], "0192a1c2-3344-7556-8677-8899aabbccdd");
        assert_eq!(line["adapter"], "claude_local");
        assert_eq!(line["code"], code);
        assert!(line.get("dropped_fields").is_none(), "{line}");
    }
    assert!(
        !text.contains("Kestrel"),
        "the event's text is never logged"
    );
}
