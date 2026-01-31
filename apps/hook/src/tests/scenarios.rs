//! X2: one test per Gherkin scenario of OrdinHook.md, named after it.

use std::io::{Cursor, Read, Write};
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use ordin_types::encoding::{b64url_encode, canonical_json};
use ordin_types::wire::hook::HookMessage;
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::RunId;
use sha2::Sha256;

use super::support::*;
use crate::app::Timings;

#[test]
fn happy_path_claude_code_tool_call_allowed() {
    let relay = Relay::start(|s, r| send(s, reply_allow(r)));
    let env = good_env(&relay.endpoint);
    let out = run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        READ_PAYLOAD,
        &quick(),
    );
    let request = relay.finish();
    assert_eq!(out.status, 0);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&out.stdout).unwrap(),
        serde_json::json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow"}})
    );
    // The MAC verifies with the session secret over the canonical JSON of the message.
    let value = serde_json::to_value(&request.message).unwrap();
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&SECRET).unwrap();
    mac.update(&canonical_json(&value));
    assert_eq!(request.mac, b64url_encode(&mac.finalize().into_bytes()));
    assert_eq!(request.protocol, 1);
    assert!(matches!(request.message, HookMessage::PreToolUse { ref tool, .. } if tool == "Read"));
}

#[test]
fn held_then_approved() {
    let relay = Relay::start(|s, r| {
        send(s, reply_holding(r, Duration::from_secs(30)));
        std::thread::sleep(Duration::from_millis(400));
        send(s, reply_allow(r));
    });
    let env = good_env(&relay.endpoint);
    let started = Instant::now();
    let out = run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        READ_PAYLOAD,
        &quick(),
    );
    relay.finish();
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert!(
        out.stdout.contains(r#""permissionDecision":"allow""#),
        "{}",
        out.stdout
    );
}

#[test]
fn held_past_the_hooks_deadline() {
    let relay = Relay::start(|s, r| {
        send(s, reply_holding(r, Duration::from_secs(120)));
        std::thread::sleep(Duration::from_millis(1500));
    });
    let env = good_env(&relay.endpoint);
    let timings = Timings {
        overall: Duration::from_millis(600),
        connect: Duration::from_millis(500),
        first_reply: Duration::from_secs(2),
    };
    let started = Instant::now();
    let out = run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        READ_PAYLOAD,
        &timings,
    );
    assert!(started.elapsed() >= Duration::from_millis(600));
    assert!(started.elapsed() < Duration::from_millis(1400));
    assert!(out.stdout.contains(r#""permissionDecision":"deny""#));
    assert!(out.stdout.contains("waiting for the person's approval"));
    relay.finish();
}

#[test]
fn core_not_running() {
    let env = good_env(&unused_endpoint("oh-none"));
    let started = Instant::now();
    let out = run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        READ_PAYLOAD,
        &quick(),
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(out.status, 0);
    assert!(out.stdout.contains("ordin is not running"));
    assert!(out.stdout.contains(r#""permissionDecision":"deny""#));
}

#[test]
fn core_slow() {
    let relay = Relay::start(|_, _| std::thread::sleep(Duration::from_millis(900)));
    let env = good_env(&relay.endpoint);
    let timings = Timings {
        overall: Duration::from_secs(5),
        connect: Duration::from_millis(500),
        first_reply: Duration::from_millis(300),
    };
    let started = Instant::now();
    let out = run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        READ_PAYLOAD,
        &timings,
    );
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert!(out.stdout.contains("ordin did not answer in time"));
    relay.finish();
}

#[test]
fn missing_secret() {
    let mut env = good_env("/nowhere");
    env.remove("ORDIN_HOOK_SECRET");
    let out = run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        READ_PAYLOAD,
        &quick(),
    );
    assert_eq!(out.status, 0);
    assert!(out.stdout.contains("ordin is not set up for this session"));
}

#[test]
fn reply_for_another_call() {
    let relay = Relay::start(|s, r| {
        let mut other = reply_allow(r);
        other["call_id"] = serde_json::json!(ordin_types::HookCallId::new().to_string());
        send(s, other);
    });
    let env = good_env(&relay.endpoint);
    let out = run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        READ_PAYLOAD,
        &quick(),
    );
    relay.finish();
    assert!(out.stdout.contains("ordin's answer could not be read"));
}

#[test]
fn copilot_payload_with_string_arguments() {
    let relay = Relay::start(|s, r| send(s, reply_allow(r)));
    let env = good_env(&relay.endpoint);
    let payload = r#"{"toolName":"bash","toolArgs":"{\"command\":\"ls\"}"}"#;
    let out = run_hook(
        &hook_args("copilot-cli", "pre-tool-use"),
        &env,
        payload,
        &quick(),
    );
    let request = relay.finish();
    match request.message {
        HookMessage::PreToolUse {
            arguments, origin, ..
        } => {
            assert_eq!(arguments, serde_json::json!({"command": "ls"}));
            assert!(matches!(origin, ToolOrigin::BuiltIn));
        }
        other => panic!("{other:?}"),
    }
    let printed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(printed["permissionDecision"], "allow");
}

#[test]
fn unknown_copilot_tool_is_not_claimed_as_built_in() {
    let relay = Relay::start(|s, r| send(s, reply_allow(r)));
    let env = good_env(&relay.endpoint);
    let payload = r#"{"toolName":"supplier_update","toolArgs":{"rate":1}}"#;
    run_hook(
        &hook_args("copilot-cli", "pre-tool-use"),
        &env,
        payload,
        &quick(),
    );
    match relay.finish().message {
        HookMessage::PreToolUse { origin, .. } => {
            assert!(
                matches!(origin, ToolOrigin::PackageMcpServer { server } if server == "unknown")
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn stop_refused() {
    let reason = "The run cannot end yet. The control judged-loop:draft is not met.";
    let relay = Relay::start(move |s, r| send(s, reply_deny(r, reason)));
    let env = good_env(&relay.endpoint);
    let payload = r#"{"hook_event_name":"Stop","stop_hook_active":false}"#;
    let out = run_hook(&hook_args("claude-code", "stop"), &env, payload, &quick());
    relay.finish();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&out.stdout).unwrap(),
        serde_json::json!({"decision": "block", "reason": reason})
    );
}

#[test]
fn stop_with_the_core_unreachable() {
    let env = good_env(&unused_endpoint("oh-none"));
    let payload = r#"{"hook_event_name":"Stop"}"#;
    let out = run_hook(&hook_args("claude-code", "stop"), &env, payload, &quick());
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, "");
}

fn mcp_env(endpoint: &str) -> std::collections::HashMap<String, String> {
    std::collections::HashMap::from([
        ("ORDIN_MCP_ENDPOINT".into(), endpoint.to_string()),
        ("ORDIN_SESSION_ID".into(), RunId::new().to_string()),
        ("ORDIN_MCP_SERVER".into(), "work".into()),
        ("ORDIN_MCP_TOKEN".into(), b64url_encode(&[9u8; 32])),
    ])
}

// On Windows this also checks SA-HOOK-PIPE-01 and -02: the bridge writes the initialise message
// while its read of the connection is pending, and the stand-in's read to the end returns only
// because the bridge closes the connection once standard input has ended.
#[test]
fn bridge_relays_bytes_unchanged() {
    let listener = Listener::bind("om");
    let endpoint = listener.endpoint.clone();
    let front = std::thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let handshake = crate::channel::read_line(&mut reader).unwrap().unwrap();
        stream.write_all(b"{\"kind\":\"ready\"}\n").unwrap();
        let init = crate::channel::read_line(&mut reader).unwrap().unwrap();
        stream
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n")
            .unwrap();
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        (handshake, init)
    });
    let env = mcp_env(&endpoint);
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n";
    let out = run_with_input(
        &args(&["mcp-bridge"]),
        &env,
        Box::new(Cursor::new(input.as_bytes().to_vec())),
        &quick(),
    );
    let (handshake, init) = front.join().unwrap();
    let handshake: serde_json::Value = serde_json::from_str(&handshake).unwrap();
    assert_eq!(handshake["protocol"], 1);
    assert_eq!(handshake["session"], env["ORDIN_SESSION_ID"]);
    assert_eq!(handshake["server"], "work");
    assert_eq!(handshake["token"], env["ORDIN_MCP_TOKEN"]);
    assert_eq!(init + "\n", input);
    assert_eq!(out.status, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
    assert!(!out.stderr.contains(&env["ORDIN_MCP_TOKEN"]));
}

#[test]
fn bridge_refused_by_the_mcp_front() {
    let listener = Listener::bind("om");
    let endpoint = listener.endpoint.clone();
    let front = std::thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let _ = crate::channel::read_line(&mut reader);
        stream
            .write_all(b"{\"kind\":\"refused\",\"code\":\"MCP_BRIDGE_UNAUTHENTICATED\"}\n")
            .unwrap();
    });
    let out = run_with_input(
        &args(&["mcp-bridge"]),
        &mcp_env(&endpoint),
        Box::new(Cursor::new(Vec::new())),
        &quick(),
    );
    front.join().unwrap();
    assert_eq!(out.status, 1);
    assert_eq!(out.stdout, "");
    assert!(out.stderr.contains("MCP_BRIDGE_UNAUTHENTICATED"));
}

/// A data root holding `run/sessions/<run>/key` with `contents` and `mode` (Unix only: Windows has no
/// mode bits, and H.2 checks them on macOS and Linux only).
pub fn key_root(contents: &[u8], mode: u32) -> (std::path::PathBuf, RunId) {
    let root = unique_path("ok");
    let run = RunId::new();
    let dir = root.join("run").join("sessions").join(run.to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("key");
    std::fs::write(&file, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = mode;
    (root, run)
}

#[test]
fn api_key_helper_prints_the_runs_key() {
    let (root, run) = key_root(b"sk-org-key\n", 0o600);
    let out = run_hook(
        &args(&[
            "api-key",
            "--run",
            &run.to_string(),
            "--data-root",
            &root.to_string_lossy(),
        ]),
        &Default::default(),
        "",
        &quick(),
    );
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, "sk-org-key\n");
    assert_eq!(out.stderr, "");
    let _ = std::fs::remove_dir_all(root);
}

// Unix only: H.2 checks the key file's mode on macOS and Linux; Windows has no mode bits.
#[cfg(unix)]
#[test]
fn api_key_file_readable_by_others() {
    let (root, run) = key_root(b"sk-org-key\n", 0o644);
    let out = run_hook(
        &args(&[
            "api-key",
            "--run",
            &run.to_string(),
            "--data-root",
            &root.to_string_lossy(),
        ]),
        &Default::default(),
        "",
        &quick(),
    );
    assert_eq!(out.status, 1);
    assert_eq!(out.stdout, "");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bad_command_line() {
    let out = run_hook(
        &args(&["--harness", "vim"]),
        &Default::default(),
        "",
        &quick(),
    );
    assert_eq!(out.status, 64);
    assert!(out.stderr.contains("usage"));
    assert_eq!(out.stdout, "");
}
