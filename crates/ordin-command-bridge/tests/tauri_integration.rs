//! Integration with Tauri (plan Phase 4, OA-5): the Bridge's plugin carried by Tauri's mock runtime
//! (`tauri::test`, feature `test`). A Service API command goes through Tauri's IPC to the Bridge and
//! the program's `ServiceApi`; the event stream goes from the real Journal through the Bridge and a
//! Tauri `Channel` opened by the WebView side (`__CHANNEL__:<id>`), whose messages are read with
//! Tauri's channel interceptor in place of a WebView. The capability check of layer 1 is Tauri's own:
//! without a grant, a plugin command is refused.
#![cfg(any(target_os = "macos", target_os = "windows"))]

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_command_bridge::{CommandBridge, IPC_COMMANDS};
use serde_json::{json, Value};
use support::*;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::utils::acl::ExecutionContext;
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow, WebviewWindowBuilder};

/// Channel messages as the WebView would receive them: (callback id, message index, JSON text).
type Received = Arc<Mutex<Vec<(u32, usize, String)>>>;

const EVENTS_CHANNEL: u32 = 7;
const STREAMS_CHANNEL: u32 = 8;

/// The origin of the program's own pages: Tauri serves them as `tauri://localhost` on macOS and as
/// `http://tauri.localhost` on Windows, and takes any other origin as remote.
#[cfg(not(windows))]
const LOCAL_ORIGIN: &str = "tauri://localhost";
#[cfg(windows)]
const LOCAL_ORIGIN: &str = "http://tauri.localhost";

fn request(cmd: &str, body: Value) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: LOCAL_ORIGIN.parse().expect("url"),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_string(),
    }
}

/// A mock app with the Bridge's plugin and state; `grant` gives window `main` every `ordin`
/// command, as the desktop program's capability `ordin:default` does.
fn app(
    bridge: CommandBridge,
    received: Received,
    grant: bool,
) -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let mut context = mock_context(noop_assets());
    if grant {
        for name in IPC_COMMANDS {
            context
                .runtime_authority_mut()
                .__allow_command(format!("plugin:ordin|{name}"), ExecutionContext::Local);
        }
    }
    let app = mock_builder()
        .plugin(CommandBridge::plugin())
        .channel_interceptor(move |_webview, callback, index, body| {
            let text = match body {
                InvokeResponseBody::Json(json) => json.clone(),
                InvokeResponseBody::Raw(bytes) => String::from_utf8_lossy(bytes).to_string(),
            };
            received.lock().unwrap().push((callback.0, index, text));
            true
        })
        .build(context)
        .expect("the mock app builds");
    app.manage(bridge);
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("window main");
    (app, window)
}

fn wait_until(limit: Duration, check: impl Fn() -> bool) -> bool {
    let end = std::time::Instant::now() + limit;
    while std::time::Instant::now() < end {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    check()
}

#[test]
fn a_service_api_command_and_its_event_stream_cross_tauri_ipc() {
    let f = tauri::async_runtime::block_on(fixture());
    let Fixture {
        bridge,
        api,
        journal,
        cancel,
        ..
    } = f;
    let bridge = Arc::try_unwrap(bridge).ok().expect("the only handle");
    let received: Received = Arc::default();
    let (_app, window) = app(bridge, Arc::clone(&received), true);

    // A Service API command: plugin:ordin|runs_stop on the local backend.
    let reply = get_ipc_response(
        &window,
        request(
            "plugin:ordin|runs_stop",
            json!({ "envelope": serde_json::from_str::<Value>(
                &envelope("runs.stop", LOCAL, json!({ "run_id": RUN }))
            ).unwrap() }),
        ),
    )
    .expect("the command answers")
    .deserialize::<Value>()
    .expect("JSON");
    assert_eq!(reply["kind"], "ok");
    assert_eq!(reply["request_id"], REQUEST);
    assert_eq!(
        api.calls(),
        vec![Call {
            method: "runs_stop",
            local_screens: true
        }]
    );

    // Its event stream: service_events with a channel the WebView side created.
    let opened = get_ipc_response(
        &window,
        request(
            "plugin:ordin|service_events",
            json!({ "after": {}, "onEvents": format!("__CHANNEL__:{EVENTS_CHANNEL}") }),
        ),
    )
    .expect("the stream opens")
    .deserialize::<Value>()
    .expect("JSON");
    assert_eq!(opened, json!({ "local_backend": LOCAL, "reset": [] }));

    tauri::async_runtime::block_on(async {
        for _ in 0..3 {
            journal
                .append_event("needs_you_changed", json!({ "workspace_id": WORKSPACE }))
                .await
                .expect("append");
        }
    });
    let events = || -> Vec<Value> {
        received
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _, _)| *id == EVENTS_CHANNEL)
            .flat_map(|(_, _, text)| {
                let batch: Value = serde_json::from_str(text).expect("a batch");
                batch["events"].as_array().cloned().unwrap_or_default()
            })
            .collect()
    };
    assert!(wait_until(Duration::from_secs(5), || events().len() == 3));
    let events = events();
    let seqs: Vec<u64> = events.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, vec![1, 2, 3]);
    for event in &events {
        assert_eq!(event["kind"], "needs_you_changed");
        assert_eq!(event["backend"], LOCAL);
        assert_eq!(event["api_version"], json!({ "major": 1, "minor": 0 }));
    }

    // The unjournalled stream: service_streams, then one message from the program.
    get_ipc_response(
        &window,
        request(
            "plugin:ordin|service_streams",
            json!({ "onStream": format!("__CHANNEL__:{STREAMS_CHANNEL}") }),
        ),
    )
    .expect("the stream opens");
    let state = window.state::<CommandBridge>();
    let session = uuid::Uuid::now_v7();
    state
        .streams()
        .send(
            session,
            ordin_command_bridge::StreamKind::Dictation,
            json!({ "kind": "partial", "text": "Please summarise" }),
        )
        .expect("queued");
    assert!(wait_until(Duration::from_secs(5), || received
        .lock()
        .unwrap()
        .iter()
        .any(|(id, _, _)| *id == STREAMS_CHANNEL)));
    let message: Value = received
        .lock()
        .unwrap()
        .iter()
        .find(|(id, _, _)| *id == STREAMS_CHANNEL)
        .map(|(_, _, text)| serde_json::from_str(text).unwrap())
        .unwrap();
    assert_eq!(message["stream"], session.to_string());
    assert_eq!(message["kind"], "dictation");

    // A malformed `after` is refused by Tauri's argument check before the Bridge sees it.
    let refused = get_ipc_response(
        &window,
        request(
            "plugin:ordin|service_events",
            json!({ "after": { "not-a-backend": 1 }, "onEvents": "__CHANNEL__:9" }),
        ),
    );
    assert!(refused.is_err());

    // service_events naming an unknown backend answers BRG_BACKEND_UNKNOWN.
    let unknown = get_ipc_response(
        &window,
        request(
            "plugin:ordin|service_events",
            json!({ "after": { "0192ffff-0000-7000-8000-000000000000": 1 },
                    "onEvents": "__CHANNEL__:10" }),
        ),
    )
    .unwrap_err();
    assert_eq!(unknown, json!({ "code": "BRG_BACKEND_UNKNOWN" }));
    cancel.cancel();
}

#[test]
fn a_command_without_the_capability_is_refused_by_tauri() {
    let f = tauri::async_runtime::block_on(fixture());
    let Fixture { bridge, api, .. } = f;
    let bridge = Arc::try_unwrap(bridge).ok().expect("the only handle");
    let (_app, window) = app(bridge, Arc::default(), false);
    let refused = get_ipc_response(
        &window,
        request(
            "plugin:ordin|runs_stop",
            json!({ "envelope": serde_json::from_str::<Value>(
                &envelope("runs.stop", LOCAL, json!({ "run_id": RUN }))
            ).unwrap() }),
        ),
    );
    let refused = refused.unwrap_err().to_string();
    assert!(refused.contains("not allowed"), "{refused}");
    // Refused for want of a grant, not for its origin: Tauri names the origin when that is the
    // reason ("not allowed on origin [...]").
    assert!(!refused.contains("origin"), "{refused}");
    assert!(api.calls().is_empty());
}

#[test]
fn every_catalogue_command_has_an_ipc_name() {
    assert_eq!(
        IPC_COMMANDS.len(),
        ordin_service_api::CATALOGUE.len() + 2,
        "one per row plus service_events and service_streams"
    );
    for meta in ordin_service_api::CATALOGUE {
        assert!(IPC_COMMANDS.contains(&meta.ipc_name), "{}", meta.ipc_name);
    }
    assert!(IPC_COMMANDS.contains(&"service_events"));
    assert!(IPC_COMMANDS.contains(&"service_streams"));
}
