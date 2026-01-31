//! The events of FormatPlugins.md *Observability* (SA-OBS-01).
//!
//! The only test in its own binary: `tracing` caches per call site whether anything listens, so a
//! test thread hitting a call site with no subscriber could leave a scoped subscriber unasked. Alone
//! in the process, no other thread logs (plan FU-017).

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use ordin_format_plugins::*;
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
fn read_and_write_log_counts_and_codes_only() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    let apm = load("apm-basic");
    let claude = load("claude");
    tracing::subscriber::with_default(subscriber, || {
        let plugins = FormatPlugins::builtin();
        // read: success, with the hook reported and counted.
        plugins
            .read_any(&apm, &request("acme", "an APM package"))
            .expect("reads");
        // read: refused after detection (a bad publisher), then before it (a bad path).
        let _ = plugins.read_any(&apm, &request("Bad Publisher", "x"));
        let _ = plugins.read_any(&tree(&[("../x", "y")]), &request("acme", "x"));
        // detect: nothing recognises it (debug).
        let _ = plugins.detect(&tree(&[("README.txt", "x")]));
        // write: success, then the two refusals.
        let parts = plugins
            .get(FormatId::Apm)
            .read(&apm, &request("acme", "x"))
            .expect("reads")
            .parts;
        plugins
            .get(FormatId::Apm)
            .write(&write_request(parts))
            .expect("writes");
        let _ = plugins
            .get(FormatId::Copilot)
            .write(&write_request(Vec::new()));
        let _ = plugins
            .get(FormatId::ClaudeCode)
            .write(&write_request(Vec::new()));
        // Reading a set-up that holds credentials must not log them.
        plugins
            .read_any(&claude, &request("acme", "a Claude Code set-up"))
            .expect("reads");
    });
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect();
    for line in &lines {
        assert!(line.get("dropped_fields").is_none(), "{line}");
    }
    let events = |name: &str| -> Vec<&serde_json::Value> {
        lines.iter().filter(|l| l["event"] == name).collect()
    };

    let reads = events("format_plugins.read");
    assert_eq!(reads.len(), 4, "{text}");
    assert_eq!(reads[0]["level"], "INFO");
    assert_eq!(reads[0]["format"], "apm");
    assert_eq!(reads[0]["declared_version"], "unversioned");
    assert_eq!(reads[0]["version_tested"], false);
    assert_eq!(reads[0]["parts"], 3);
    assert_eq!(reads[0]["mapping_items"], 1);
    assert!(reads[0]["duration_ms"].is_number());
    assert!(reads[0].get("error_code").is_none());
    assert_eq!(reads[1]["level"], "WARNING");
    assert_eq!(reads[1]["format"], "apm");
    assert_eq!(reads[1]["error_code"], "FMT_PART_INVALID");
    assert_eq!(reads[2]["level"], "WARNING");
    assert_eq!(reads[2]["error_code"], "FMT_PATH_INVALID");
    assert!(reads[2].get("format").is_none());
    assert_eq!(reads[3]["format"], "claude_code");

    let writes = events("format_plugins.write");
    assert_eq!(writes.len(), 3, "{text}");
    assert_eq!(writes[0]["level"], "INFO");
    assert_eq!(writes[0]["format"], "apm");
    assert_eq!(writes[0]["parts"], 3);
    assert_eq!(writes[0]["starting_files"], 1);
    assert_eq!(writes[0]["loss_items"], 0);
    assert_eq!(writes[1]["level"], "WARNING");
    assert_eq!(writes[1]["format"], "copilot");
    assert_eq!(writes[1]["error_code"], "FMT_WRITE_UNSUPPORTED");
    assert_eq!(writes[2]["format"], "claude_code");

    // Debug is below the default level of the formatter's filter-less layer: the detect event is
    // written, with the formats found and nothing else.
    let detects = events("format_plugins.detect");
    assert_eq!(detects.len(), 4, "{text}");
    assert_eq!(detects[0]["formats"], "apm");

    // Never file content, a path, or a credential.
    for forbidden in [
        "placeholder-not-a-secret",
        "JIRA_TOKEN",
        "pre-commit",
        "Write in plain",
        ".apm/",
    ] {
        assert!(
            !text.contains(forbidden),
            "the log holds {forbidden}: {text}"
        );
    }
}
