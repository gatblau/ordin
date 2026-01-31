//! The C-73 spike (CommandBridge.md *Constants and rules*, *Performance*; Playbook Step 34 gate):
//! 1,000 events a second for 60 s through the real Journal, the Bridge and a Tauri channel of the mock
//! runtime, with no `BRG_WINDOW_LAGGING`; and local command round trips at p95 with a 4 KiB envelope.
//!
//! Ignored by default because it is a reference-machine measurement that takes a minute, not because
//! it may fail; run it with
//! `cargo test -p ordin-command-bridge --release --test performance -- --ignored --nocapture`.
#![cfg(any(target_os = "macos", target_os = "windows"))]

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ordin_command_bridge::{CommandBridge, IPC_COMMANDS};
use serde_json::{json, Value};
use support::*;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets};
use tauri::utils::acl::ExecutionContext;
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindowBuilder};
use tracing_subscriber::layer::SubscriberExt;

const RATE: u64 = 1_000;
const SECONDS: u64 = 60;
const CHANNEL: u32 = 3;

fn request(cmd: &str, body: Value) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "tauri://localhost".parse().expect("url"),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_string(),
    }
}

fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[(samples.len() * 95).div_ceil(100).saturating_sub(1)]
}

#[test]
#[ignore = "C-73 reference-machine measurement, 60 s; run with --ignored"]
fn c73_one_thousand_events_a_second_for_sixty_seconds() {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    tracing::subscriber::set_global_default(subscriber).expect("install the subscriber");

    let f = tauri::async_runtime::block_on(fixture());
    let Fixture {
        bridge,
        journal,
        cancel,
        ..
    } = f;
    let bridge = Arc::try_unwrap(bridge).ok().expect("the only handle");

    // Each channel message is timed as it reaches the (intercepted) WebView side.
    let received: Arc<Mutex<Vec<(Instant, String)>>> = Arc::default();
    let sink = Arc::clone(&received);
    let mut context = mock_context(noop_assets());
    for name in IPC_COMMANDS {
        context
            .runtime_authority_mut()
            .__allow_command(format!("plugin:ordin|{name}"), ExecutionContext::Local);
    }
    let app = mock_builder()
        .plugin(CommandBridge::plugin())
        .channel_interceptor(move |_webview, callback, _index, body| {
            if callback.0 == CHANNEL {
                if let InvokeResponseBody::Json(json) = body {
                    sink.lock().unwrap().push((Instant::now(), json.clone()));
                }
            }
            true
        })
        .build(context)
        .expect("the mock app builds");
    app.manage(bridge);
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("window main");

    // Command round trips, 4 KiB envelopes, before the event load.
    let mut input = json!({ "run_id": RUN });
    let base = envelope("runs.stop", LOCAL, input.clone()).len();
    input["pad"] = json!("x".repeat(4_096 - base - 10));
    let body: Value = serde_json::from_str(&envelope("runs.stop", LOCAL, input)).unwrap();
    let mut round_trips = Vec::new();
    for _ in 0..1_000 {
        let started = Instant::now();
        get_ipc_response(
            &window,
            request("plugin:ordin|runs_stop", json!({ "envelope": body })),
        )
        .expect("a reply");
        round_trips.push(started.elapsed());
    }
    let command_p95 = p95(round_trips);

    get_ipc_response(
        &window,
        request(
            "plugin:ordin|service_events",
            json!({ "after": {}, "onEvents": format!("__CHANNEL__:{CHANNEL}") }),
        ),
    )
    .expect("the stream opens");

    // The producer: one Journal event every millisecond for 60 s.
    let appended_at: Arc<Mutex<Vec<Instant>>> = Arc::default();
    let stamps = Arc::clone(&appended_at);
    let started = Instant::now();
    tauri::async_runtime::block_on(async move {
        let mut tick = tokio::time::interval(Duration::from_micros(1_000_000 / RATE));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
        for _ in 0..RATE * SECONDS {
            tick.tick().await;
            journal
                .append_event("needs_you_changed", json!({ "workspace_id": WORKSPACE }))
                .await
                .expect("append");
            stamps.lock().unwrap().push(Instant::now());
        }
    });
    let produced_in = started.elapsed();
    let total = (RATE * SECONDS) as usize;
    let count = || -> usize {
        received
            .lock()
            .unwrap()
            .iter()
            .map(|(_, text)| {
                serde_json::from_str::<Value>(text).unwrap()["events"]
                    .as_array()
                    .map_or(0, Vec::len)
            })
            .sum()
    };
    let end = Instant::now() + Duration::from_secs(10);
    while count() < total && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }

    let messages = received.lock().unwrap().clone();
    let appended = appended_at.lock().unwrap().clone();
    let mut seqs = Vec::with_capacity(total);
    let mut latencies = Vec::with_capacity(total);
    let mut resets = 0usize;
    for (at, text) in &messages {
        let batch: Value = serde_json::from_str(text).unwrap();
        for event in batch["events"].as_array().unwrap() {
            if event["kind"] == "reset" {
                resets += 1;
                continue;
            }
            let seq = event["seq"].as_u64().unwrap();
            seqs.push(seq);
            if let Some(appended) = appended.get(seq as usize - 1) {
                latencies.push(at.saturating_duration_since(*appended));
            }
        }
    }
    let event_p95 = p95(latencies);
    let lagging = capture.of("brg.window_lagging").len();
    println!(
        "C-73: {} events produced in {:.1} s ({:.0}/s); {} delivered in {} batches; resets {}; \
         BRG_WINDOW_LAGGING lines {}; journal-to-WebView p95 {:.2} ms; command round trip p95 \
         {:.3} ms (4 KiB envelope, 1,000 calls)",
        total,
        produced_in.as_secs_f64(),
        total as f64 / produced_in.as_secs_f64(),
        seqs.len(),
        messages.len(),
        resets,
        lagging,
        event_p95.as_secs_f64() * 1_000.0,
        command_p95.as_secs_f64() * 1_000.0,
    );
    assert_eq!(lagging, 0, "no BRG_WINDOW_LAGGING");
    assert_eq!(resets, 0, "no reset was sent");
    assert_eq!(seqs.len(), total, "every event reached the window");
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "in seq order");
    assert!(
        produced_in <= Duration::from_secs(SECONDS + 1),
        "the load was 1,000 events a second"
    );
    // CommandBridge.md *Performance*: a local event reaches the WebView within 50 ms p95; the Bridge
    // and IPC take at most 10 ms p95 of a command's round trip.
    assert!(event_p95 <= Duration::from_millis(50), "{event_p95:?}");
    assert!(command_p95 <= Duration::from_millis(10), "{command_p95:?}");
    cancel.cancel();
}
