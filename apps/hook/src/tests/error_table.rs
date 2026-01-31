//! X1: one test per Error Table row of OrdinHook.md, named `row_<code>`.

use std::time::Duration;

use ordin_types::encoding::b64url_encode;
use ordin_types::RunId;

use super::scenarios::key_root;
use super::support::*;
use crate::app::{Io, Timings};

type Script = Box<dyn FnOnce(&mut Stream, &ordin_types::wire::hook::HookRequest) + Send>;

const DENY: &str = r#""permissionDecision":"deny""#;

fn pre(env: &std::collections::HashMap<String, String>, payload: &str) -> Outcome {
    run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        env,
        payload,
        &quick(),
    )
}

#[test]
fn row_hook_usage() {
    let out = run_hook(&args(&["--bogus"]), &Default::default(), "", &quick());
    assert_eq!(out.status, 64);
    assert!(out.stderr.contains("ordin-hook --harness"));
}

#[test]
fn row_hook_env_invalid() {
    let mut env = good_env("/nowhere");
    env.insert("ORDIN_HOOK_PROTOCOL".into(), "2".into());
    let out = pre(&env, READ_PAYLOAD);
    assert_eq!(out.status, 0);
    assert!(
        out.stdout.contains(DENY) && out.stdout.contains("ordin is not set up for this session")
    );
}

#[test]
fn row_hook_payload_invalid() {
    let env = good_env("/nowhere");
    for payload in ["not json", "[]", r#"{"tool_name":"Read","tool_input":"x"}"#] {
        let out = pre(&env, payload);
        assert_eq!(out.status, 0);
        assert!(
            out.stdout.contains("ordin could not read this tool call"),
            "{payload}"
        );
    }
    let long = format!(r#"{{"tool_name":"{}","tool_input":{{}}}}"#, "t".repeat(129));
    assert!(pre(&env, &long)
        .stdout
        .contains("ordin could not read this tool call"));
}

#[test]
fn row_hook_payload_invalid_for_input_over_the_cap() {
    let env = good_env("/nowhere");
    let big = vec![b' '; crate::app::STDIN_MAX as usize + 1];
    let out = run_with_input(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        Box::new(std::io::Cursor::new(big)),
        &quick(),
    );
    assert!(out.stdout.contains("ordin could not read this tool call"));
}

#[test]
fn row_hook_request_too_large() {
    let env = good_env("/nowhere");
    let payload = format!(
        r#"{{"tool_name":"Write","tool_input":{{"content":"{}"}}}}"#,
        "a".repeat(1_100_000)
    );
    let out = pre(&env, &payload);
    assert_eq!(out.status, 0);
    assert!(out
        .stdout
        .contains("this tool call is too large for ordin to check"));
}

#[test]
fn row_hook_core_unreachable() {
    let env = good_env(&unused_endpoint("oh-none"));
    assert!(pre(&env, READ_PAYLOAD)
        .stdout
        .contains("ordin is not running"));
}

#[test]
fn row_hook_core_timeout() {
    let relay = Relay::start(|_, _| std::thread::sleep(Duration::from_millis(600)));
    let env = good_env(&relay.endpoint);
    let timings = Timings {
        overall: Duration::from_secs(5),
        connect: Duration::from_millis(500),
        first_reply: Duration::from_millis(200),
    };
    let out = run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        READ_PAYLOAD,
        &timings,
    );
    relay.finish();
    assert!(out.stdout.contains("ordin did not answer in time"));
}

#[test]
fn row_hook_reply_malformed() {
    let cases: Vec<Script> = vec![
        // Unreadable.
        Box::new(|s, _| send(s, serde_json::json!({"kind": "maybe"}))),
        // Closed without a reply.
        Box::new(|_, _| {}),
        // Wrong kind for the event.
        Box::new(|s, r| send(s, reply_ack(r))),
        // A second holding.
        Box::new(|s, r| {
            send(s, reply_holding(r, Duration::from_secs(30)));
            send(s, reply_holding(r, Duration::from_secs(30)));
        }),
    ];
    for case in cases {
        let relay = Relay::start(case);
        let env = good_env(&relay.endpoint);
        let out = pre(&env, READ_PAYLOAD);
        relay.finish();
        assert!(
            out.stdout.contains("ordin's answer could not be read"),
            "{}",
            out.stdout
        );
    }
}

#[test]
fn row_hook_approval_pending() {
    let relay = Relay::start(|s, r| {
        send(s, reply_holding(r, Duration::from_millis(150)));
        std::thread::sleep(Duration::from_millis(500));
    });
    let env = good_env(&relay.endpoint);
    let out = pre(&env, READ_PAYLOAD);
    relay.finish();
    assert!(out.stdout.contains("waiting for the person's approval"));
}

#[test]
fn row_relay_denies() {
    let relay = Relay::start(|s, r| send(s, reply_deny(r, "blocked by rule 7")));
    let env = good_env(&relay.endpoint);
    let out = pre(&env, READ_PAYLOAD);
    relay.finish();
    assert_eq!(out.status, 0);
    assert!(out.stdout.contains(DENY) && out.stdout.contains("blocked by rule 7"));
}

#[test]
fn row_hook_output_failed() {
    let env = good_env("/nowhere");
    let err = Sink::default();
    let io = Io {
        stdin: Box::new(std::io::Cursor::new(READ_PAYLOAD.as_bytes().to_vec())),
        stdout: Box::new(Broken),
        stderr: Box::new(err.clone()),
    };
    let get = |name: &str| env.get(name).cloned();
    let status = crate::app::run(
        &hook_args("claude-code", "pre-tool-use"),
        &get,
        io,
        &quick(),
    );
    assert_eq!(status, 2);
    assert!(!err.text().is_empty());
}

#[test]
fn row_hook_bridge_env_invalid() {
    let env = std::collections::HashMap::from([
        ("ORDIN_MCP_ENDPOINT".to_string(), "/x".to_string()),
        ("ORDIN_SESSION_ID".to_string(), RunId::new().to_string()),
        ("ORDIN_MCP_SERVER".to_string(), "Bad Name".to_string()),
        ("ORDIN_MCP_TOKEN".to_string(), b64url_encode(&[1u8; 32])),
    ]);
    let out = run_hook(&args(&["mcp-bridge"]), &env, "", &quick());
    assert_eq!(out.status, 1);
    assert_eq!(out.stdout, "");
    assert!(out.stderr.contains("ordin is not set up for this session"));
}

#[test]
fn row_hook_bridge_refused() {
    // The front accepts, then closes without `ready`: broken.
    let listener = Listener::bind("om");
    let endpoint = listener.endpoint.clone();
    let front = std::thread::spawn(move || {
        let _ = listener.accept();
    });
    let env = std::collections::HashMap::from([
        ("ORDIN_MCP_ENDPOINT".to_string(), endpoint),
        ("ORDIN_SESSION_ID".to_string(), RunId::new().to_string()),
        ("ORDIN_MCP_SERVER".to_string(), "work".to_string()),
        ("ORDIN_MCP_TOKEN".to_string(), b64url_encode(&[1u8; 32])),
    ]);
    let out = run_hook(&args(&["mcp-bridge"]), &env, "", &quick());
    front.join().unwrap();
    assert_eq!(out.status, 1);
    assert_eq!(out.stdout, "");
    assert!(out.stderr.contains("HOOK_BRIDGE_REFUSED"));
}

#[test]
fn row_hook_key_unreadable() {
    let (root, run) = key_root(b"\n", 0o600);
    let missing = RunId::new();
    for target in [run, missing] {
        let out = run_hook(
            &args(&[
                "api-key",
                "--run",
                &target.to_string(),
                "--data-root",
                &root.to_string_lossy(),
            ]),
            &Default::default(),
            "",
            &quick(),
        );
        assert_eq!(out.status, 1);
        assert_eq!(out.stdout, "");
    }
    let _ = std::fs::remove_dir_all(root);
}
