//! The log events of ResolverAndCompiler.md *Observability* (SA-OBS-01; Logging.md R-2).
//!
//! The only test in its own binary: `tracing` caches per call site whether anything listens, and another
//! test thread hitting the same call site with no subscriber could leave this test's scoped subscriber
//! unasked (plan FU-017). Alone in the process, no other thread logs.

mod common;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use common::*;
use ordin_agent_model::{PartKind, PartSourceError};
use ordin_resolver::{compile_instructions, ResolverAndCompiler};
use serde_json::Value;
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

fn lines(buffer: &Buffer) -> Vec<Value> {
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    text.lines()
        .map(|l| serde_json::from_str(l).expect("a JSON line"))
        .collect()
}

fn named<'a>(lines: &'a [Value], event: &str) -> Vec<&'a Value> {
    lines.iter().filter(|l| l["event"] == event).collect()
}

#[test]
fn every_event_is_written_once_with_counts_and_identifiers_only() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let dir = TempDir::new();
    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            // A deprecated constituent and a pack candidate that resolves to nothing.
            let mut cat = Cat::basic(&format!(
                "requiredPracticePacks:\n  - atLeast: 1\n    anyOf:\n      - urn:ordin:t:practice-packs:stack:missing\n      - {GO}\nfixedPracticePacks:\n  - {GO}\n"
            ));
            cat.with_packs();
            cat.md("rule", RULE_A, "1.0.0", "  status: deprecated\n", "", "Secret rule text.");
            let resolver = bound(&dir, memory(&cat, vec![]));
            let first = resolver.build(&request(ROOT)).await.unwrap();
            resolver.build(&request(ROOT)).await.unwrap();
            let failure = resolver.resolve(&request("urn:ordin:t:workspaces:missing")).await;
            assert!(failure.is_err());
            resolver.prune(&BTreeSet::new()).unwrap();
            assert!(first.dir.parent().is_some());

            // A call before the source is bound.
            let unbound = ResolverAndCompiler::new(config(&dir)).unwrap();
            assert!(unbound.resolve(&request(ROOT)).await.is_err());

            // Tampered files, and the instruction text.
            let mut tampered = FaultySource::new(memory(&Cat::basic(""), vec![]));
            tampered.tamper_files = true;
            assert!(compile_instructions(&tampered, &request(ROOT)).await.is_err());
            let plain = memory(&Cat::basic(""), vec![]);
            compile_instructions(&plain, &request(ROOT)).await.unwrap();

            // A source that cannot be read.
            let mut locked = FaultySource::new(memory(&Cat::basic(""), vec![]));
            locked.resolve_error = Some(PartSourceError::Unavailable { retryable: true });
            assert!(compile_instructions(&locked, &request(ROOT)).await.is_err());
        });
    });
    let lines = lines(&buffer);

    let resolved = named(&lines, "resolver.resolved");
    assert_eq!(resolved.len(), 4, "two builds and two instruction compiles");
    let first = resolved[0];
    assert_eq!(first["level"], "INFO");
    assert_eq!(first["root_id"], ROOT);
    assert_eq!(first["root_version"], "1.0.0");
    assert!(first["plan_hash"].as_str().unwrap().starts_with("sha256:"));
    assert_eq!(first["packs"], 1);
    assert_eq!(first["overlays"], 0);
    assert_eq!(first["pending_choices"], 0);
    assert_eq!(first["warnings"], 1);
    assert!(first["parts"].as_u64().unwrap() >= 5);
    assert!(first.get("duration_ms").is_some());

    let compiled = named(&lines, "resolver.compiled");
    assert_eq!(compiled.len(), 2);
    assert_eq!(compiled[0]["reused"], false);
    assert_eq!(compiled[1]["reused"], true);
    assert!(compiled[0]["bytes"].as_u64().unwrap() > 0);

    let failed = named(&lines, "resolver.resolve_failed");
    assert_eq!(failed.len(), 3);
    assert_eq!(failed[0]["level"], "WARNING");
    assert_eq!(failed[0]["error_code"], "RES_ROOT_NOT_FOUND");
    assert_eq!(failed[0]["retryable"], false);
    assert_eq!(failed[0]["root_id"], "urn:ordin:t:workspaces:missing");
    assert_eq!(failed[1]["error_code"], "RES_DIGEST_MISMATCH");
    assert_eq!(failed[2]["error_code"], "RES_SOURCE_UNAVAILABLE");
    assert_eq!(failed[2]["retryable"], true);

    let deprecated = named(&lines, "resolver.constituent_deprecated");
    assert!(!deprecated.is_empty());
    assert_eq!(deprecated[0]["part_id"], RULE_A);
    assert_eq!(
        named(&lines, "resolver.pack_candidate_missing")[0]["part_id"],
        "urn:ordin:t:practice-packs:stack:missing"
    );

    let mismatch = named(&lines, "resolver.digest_mismatch");
    assert_eq!(mismatch.len(), 1);
    assert_eq!(mismatch[0]["level"], "ERROR");
    assert_eq!(mismatch[0]["repository_label"], "bundled");

    let unbound = named(&lines, "resolver.source_not_bound");
    assert_eq!(unbound.len(), 1);
    assert_eq!(unbound[0]["level"], "ERROR");
    assert_eq!(unbound[0]["operation"], "resolve");

    let pruned = named(&lines, "resolver.pruned");
    assert_eq!(pruned.len(), 1);
    assert_eq!(pruned[0]["removed"], 1);
    assert_eq!(pruned[0]["kept"], 0);
    assert!(pruned[0]["bytes_freed"].as_u64().unwrap() > 0);

    let instructions = named(&lines, "resolver.instructions_compiled");
    assert_eq!(instructions.len(), 1);
    assert_eq!(instructions[0]["parts"], 3);

    // Never part content or file paths.
    let all = serde_json::to_string(&lines).unwrap();
    assert!(!all.contains("Secret rule text"));
    assert!(!all.contains(dir.path().to_str().unwrap()));
    let _ = PartKind::Rule;
}
