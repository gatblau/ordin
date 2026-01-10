//! The *Performance targets* of RuntimeAdapterInterface.md, measured with `std::time::Instant` as
//! `ordin-types` measures its own (no bench crate: the workspace names none, and a p95 over single
//! calls needs only a clock).
//!
//! Each target is timed call by call over many samples after a warm-up; the run prints the p50 and
//! p95 of each and exits with status 1 when a p95 is over its target.

use std::hint::black_box;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use futures::stream::{self, StreamExt};
use ordin_runtime_adapter::*;
use ordin_types::{AgentInstanceId, RunId, WorkspaceId};

const WARM_UP: usize = 2_000;
const SAMPLES: usize = 20_000;

/// p50 and p95 of `samples` timed calls of `call`.
fn measure(samples: usize, mut call: impl FnMut()) -> (Duration, Duration) {
    for _ in 0..WARM_UP {
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

fn report(name: &str, (p50, p95): (Duration, Duration), target: Duration) -> bool {
    let ok = p95 <= target;
    println!(
        "{name}: p50 {p50:?}, p95 {p95:?} (target {target:?}) {}",
        if ok { "ok" } else { "OVER" }
    );
    ok
}

fn main() {
    let run_id = RunId::parse("0192a1c2-3344-7556-8677-8899aabbccdd").unwrap();
    let req = StartRun {
        run_id,
        workspace_id: WorkspaceId::parse("0192a0f1-1111-7222-8333-444455556666").unwrap(),
        target: RunTarget::AgentInstance {
            agent_instance_id: AgentInstanceId::parse("0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff")
                .unwrap(),
        },
        input: StartInput::Command {
            name: "compare-quotes".into(),
            arguments: "Kestrel quote ".repeat(1_171)[..16_384].to_string(),
        },
        attachments: vec![],
        limits: RunLimits {
            wall_clock_secs: 3600,
            request_budget: None,
            idle_timeout_secs: 600,
        },
        thread: None,
        model: None,
        thinking: None,
        caller: ToolCaller::Agent,
        scope_id: None,
        coding_task_id: None,
    };
    let start = measure(SAMPLES, || {
        black_box(check_start(AdapterKind::ClaudeLocal, black_box(&req))).unwrap();
    });

    // One event per poll: a stream of progress events, each `next()` timed alone.
    let events: Vec<RunEvent> = std::iter::once(RunEvent::Started {
        runtime_session_id: Some("session-1".into()),
        remote_thread_id: None,
    })
    .chain((0..WARM_UP + SAMPLES).map(|done| RunEvent::Progress {
        progress: RunProgress {
            line: "Read supplier quotes".into(),
            count: Some(ProgressCount {
                done: done as u64,
                total: None,
            }),
        },
    }))
    .collect();
    let mut guarded = guard_events(
        AdapterKind::ClaudeLocal,
        run_id,
        stream::iter(events).boxed(),
    );
    let guard = measure(SAMPLES, || {
        black_box(block_on(guarded.next())).unwrap();
    });

    let ok = report(
        "check_start, 16,384-character arguments",
        start,
        Duration::from_micros(50),
    ) & report("guard_events, per event", guard, Duration::from_micros(10));
    if !ok {
        std::process::exit(1);
    }
}
