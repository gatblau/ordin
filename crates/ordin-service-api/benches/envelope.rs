//! The *Performance targets* of ServiceApi.md, measured with `std::time::Instant` (as `ordin-types`
//! does): `accept` of a 4 KiB and of a 1 MiB envelope, `encode_for_transport` of a 4 KiB event, and
//! `check_block` of a 10,000-row table without reference resolution.
//!
//! Each target is timed call by call after a warm-up; the run prints the p50 and p95 of each and
//! exits with status 1 when a p95 is over its target.

use std::hint::black_box;
use std::time::{Duration, Instant};

use ordin_service_api::blocks::*;
use ordin_service_api::*;
use ordin_types::prelude::*;
use ordin_types::wire::ContentRef;
use serde_json::{json, Value};

const BACKEND: &str = "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b";

/// p50 and p95 of `samples` timed calls of `call`, after `warm_up` untimed ones.
fn measure(warm_up: usize, samples: usize, mut call: impl FnMut()) -> (Duration, Duration) {
    for _ in 0..warm_up {
        call();
    }
    let mut times: Vec<Duration> = (0..samples)
        .map(|_| {
            let start = Instant::now();
            call();
            start.elapsed()
        })
        .collect();
    times.sort();
    (times[samples / 2], times[samples * 95 / 100])
}

/// A `files.save_edit` envelope whose body is about `bytes` long (text-save commands take up to
/// 9 MiB, so a 1 MiB body is admitted).
fn save_edit(bytes: usize) -> Vec<u8> {
    let text = "Résumé des coûts, déjà validé.\n".repeat(bytes / 33 + 1);
    let text: String = text.chars().take(bytes.saturating_sub(300)).collect();
    serde_json::to_vec(&json!({
        "api_version": {"major": 1, "minor": 0},
        "request_id": "0192a1c0-0d1e-7f20-8a31-425364758697",
        "command": "files.save_edit",
        "backend": BACKEND,
        "input": {"place_id": "0192a0f2-2222-7333-8444-555566667777", "path": "Q3/notes.md", "text": text},
    }))
    .unwrap_or_default()
}

struct NoLookups;

#[async_trait::async_trait]
impl RefResolver for NoLookups {
    async fn content(&self, _: WorkspaceId, _: &ContentRef) -> bool {
        true
    }
    async fn email_message(&self, _: WorkspaceId, _: EmailLinkId, _: &str) -> bool {
        true
    }
    async fn entity(&self, _: WorkspaceId, _: EntityRef) -> bool {
        true
    }
    async fn run_agent_name(&self, _: RunId) -> Option<String> {
        None
    }
    async fn plan_state(&self, _: RunId) -> PlanState {
        PlanState::default()
    }
    fn approval_verbs(&self) -> &[String] {
        &[]
    }
}

fn big_table() -> Value {
    let rows: Vec<Value> = (0..10_000)
        .map(|i| json!([format!("Supplier {i}"), 400.0 + f64::from(i), 0.01]))
        .collect();
    // A company-system source: the timing is the check without reference resolution.
    json!({"kind":"table","title":"Rates","columns":[{"name":"Supplier","type":"text"},{"name":"Rate","type":"money","currency":"GBP"},{"name":"Change","type":"percentage"}],"rows":rows,"source":{"kind":"company_system","system":"Finance ledger","read_at":"2026-01-10T15:20:00.000Z"}})
}

fn main() {
    let backend = BackendId::parse(BACKEND).unwrap_or_default();
    let groups: Vec<&'static str> = CATALOGUE.iter().map(|m| m.module).collect();
    let small = save_edit(4 * 1024);
    let large = save_edit(1024 * 1024);
    let incoming = |body: &'static [u8]| Incoming {
        body,
        routed_name: "files.save_edit",
        origin: CallOrigin::LocalScreens,
        this_backend: backend,
        groups: Box::leak(groups.clone().into_boxed_slice()),
    };
    let small_in = incoming(Box::leak(small.into_boxed_slice()));
    let large_in = incoming(Box::leak(large.into_boxed_slice()));

    let event = EventEnvelope::new(
        backend,
        1,
        Timestamp::now(),
        "block_shown",
        json!({"workspace_id": WorkspaceId::new(), "block_id": BlockId::new(),
               "block": {"kind": "message", "text": "x".repeat(3_800), "attachments": []}}),
    );
    let Ok(event) = event else {
        eprintln!("the bench event did not decode");
        std::process::exit(1);
    };

    let table = big_table();
    let ctx = BlockCheckContext {
        workspace_id: WorkspaceId::new(),
        run_id: None,
        now: Timestamp::now(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let Ok(runtime) = runtime else {
        eprintln!("no runtime");
        std::process::exit(1);
    };

    let targets: [(&str, Duration, (Duration, Duration)); 4] = [
        (
            "accept 4 KiB",
            Duration::from_micros(200),
            measure(500, 5_000, || {
                black_box(accept(black_box(&small_in)).is_ok());
            }),
        ),
        (
            "accept 1 MiB",
            Duration::from_millis(5),
            measure(10, 100, || {
                black_box(accept(black_box(&large_in)).is_ok());
            }),
        ),
        (
            "encode_for_transport 4 KiB",
            Duration::from_micros(100),
            measure(500, 5_000, || {
                black_box(encode_for_transport(black_box(&event)).is_ok());
            }),
        ),
        (
            "check_block 10,000-row table",
            Duration::from_millis(20),
            measure(3, 40, || {
                let outcome = runtime.block_on(check_block(black_box(&table), &ctx, &NoLookups));
                black_box(outcome.is_ok());
            }),
        ),
    ];

    let mut over = false;
    for (name, target, (p50, p95)) in targets {
        let verdict = if p95 <= target { "ok" } else { "OVER" };
        println!("{name:<30} p50 {p50:>10.2?}  p95 {p95:>10.2?}  target {target:>8.2?}  {verdict}");
        over |= p95 > target;
    }
    if over {
        std::process::exit(1);
    }
}
