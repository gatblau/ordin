//! Unit tests of the command line, environment, MAC, channel, bridge and key modules.

use std::io::Write;
use std::time::Duration;

use ordin_types::encoding::b64url_encode;
use ordin_types::wire::hook::{HarnessKind, HookMessage, HookReply};
use ordin_types::{HookCallId, RunId};

use super::scenarios::key_root;
use super::support::*;
use crate::app::{map_reply, version_line};
use crate::cli::{parse, Command};
use crate::event::Event;
use crate::{api_key, bridge, env, mac};

#[test]
fn cli_parses_each_mode() {
    let hook = parse(&hook_args("copilot-cli", "stop")).unwrap();
    assert!(matches!(
        hook,
        Command::Hook {
            harness: HarnessKind::CopilotCli,
            event: Event::Stop,
            ..
        }
    ));
    assert!(matches!(
        parse(&hook_args("claude-code", "post-tool-use")),
        Ok(Command::Hook {
            event: Event::PostToolUse,
            ..
        })
    ));
    assert_eq!(parse(&args(&["mcp-bridge"])), Ok(Command::McpBridge));
    assert_eq!(
        parse(&args(&[
            "mcp-bridge",
            "work",
            "--agent-instance",
            "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"
        ])),
        Ok(Command::McpBridge)
    );
    assert!(parse(&args(&["mcp-bridge", "work"])).is_err());
    assert!(parse(&args(&["mcp-bridge", "work", "--agent-instance", "nope"])).is_err());
    assert_eq!(parse(&args(&["--version"])), Ok(Command::Version));
    let run = RunId::new().to_string();
    // An absolute folder on this platform: a Windows path needs a drive.
    let data_root = if cfg!(windows) { r"C:\data" } else { "/data" };
    let parsed = parse(&args(&["api-key", "--run", &run, "--data-root", data_root])).unwrap();
    assert!(matches!(
        parsed,
        Command::ApiKey {
            data_root: Some(_),
            ..
        }
    ));
    assert!(matches!(
        parse(&args(&["api-key", "--run", &run])),
        Ok(Command::ApiKey {
            data_root: None,
            ..
        })
    ));
}

#[test]
fn cli_refuses_bad_command_lines() {
    let run = RunId::new().to_string();
    let agent = ordin_types::AgentInstanceId::new().to_string();
    let bad: Vec<Vec<&str>> = vec![
        vec![],
        vec![
            "--harness",
            "vim",
            "--event",
            "stop",
            "--agent-instance",
            &agent,
        ],
        vec![
            "--harness",
            "claude-code",
            "--event",
            "later",
            "--agent-instance",
            &agent,
        ],
        vec![
            "--harness",
            "claude-code",
            "--event",
            "stop",
            "--agent-instance",
            "nope",
        ],
        vec!["--harness", "claude-code", "--event", "stop"],
        vec![
            "--harness",
            "claude-code",
            "--event",
            "stop",
            "--agent-instance",
        ],
        vec![
            "--harness",
            "claude-code",
            "--harness",
            "claude-code",
            "--event",
            "stop",
            "--agent-instance",
            &agent,
        ],
        vec![
            "--harness",
            "claude-code",
            "--event",
            "stop",
            "--agent-instance",
            &agent,
            "--extra",
            "1",
        ],
        vec!["mcp-bridge", "extra"],
        vec!["--version", "extra"],
        vec!["api-key"],
        vec!["api-key", "--run", "nope"],
        vec!["api-key", "--run", &run, "--data-root", "relative/path"],
        vec!["api-key", "--run", &run, "--bogus", "x"],
    ];
    for line in bad {
        let failure = parse(&args(&line)).expect_err(&format!("{line:?}"));
        assert_eq!(failure.code, "HOOK_USAGE");
    }
}

#[test]
fn version_prints_the_protocol() {
    let out = run_hook(&args(&["--version"]), &Default::default(), "", &quick());
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout.trim(), version_line());
    assert!(out.stdout.contains("protocol 1"));
}

#[test]
fn environment_is_validated() {
    let good = good_env("/run/hook.sock");
    let read =
        |m: &std::collections::HashMap<String, String>| env::read(&|n: &str| m.get(n).cloned());
    let ok = read(&good).unwrap();
    assert_eq!(ok.secret, SECRET);
    assert_eq!(ok.protocol, 1);
    let tweaks: Vec<(&str, Option<String>)> = vec![
        ("ORDIN_HOOK_ENDPOINT", None),
        ("ORDIN_HOOK_ENDPOINT", Some("relative.sock".into())),
        ("ORDIN_HOOK_ENDPOINT", Some(format!("/{}", "a".repeat(120)))),
        ("ORDIN_HOOK_SECRET", None),
        ("ORDIN_HOOK_SECRET", Some(b64url_encode(&[1u8; 31]))),
        (
            "ORDIN_HOOK_SECRET",
            Some(format!("{}=", b64url_encode(&[1u8; 32]))),
        ),
        ("ORDIN_SESSION_ID", None),
        ("ORDIN_SESSION_ID", Some("nope".into())),
        ("ORDIN_HOOK_PROTOCOL", None),
        ("ORDIN_HOOK_PROTOCOL", Some("2".into())),
    ];
    for (name, value) in tweaks {
        let mut m = good.clone();
        match &value {
            Some(v) => m.insert(name.to_string(), v.clone()),
            None => m.remove(name),
        };
        let long_ok = !cfg!(target_os = "macos")
            && name == "ORDIN_HOOK_ENDPOINT"
            && value.as_deref().is_some_and(|v| v.len() > 100);
        if long_ok {
            assert!(read(&m).is_ok());
        } else {
            assert_eq!(
                read(&m).err().map(|f| f.code),
                Some("HOOK_ENV_INVALID"),
                "{name}"
            );
        }
    }
    assert!(env::endpoint_ok(r"\\.\pipe\ordin-hook-abc"));
    assert!(!env::endpoint_ok(r"\\.\pipe\"));
}

#[test]
fn mac_is_stable_and_depends_on_the_secret_and_message() {
    let message = HookMessage::Stop {
        harness: HarnessKind::ClaudeCode,
    };
    let a = mac::sign(&SECRET, &message);
    assert_eq!(a, mac::sign(&SECRET, &message));
    assert_eq!(a.len(), 43);
    assert_ne!(a, mac::sign(&[8u8; 32], &message));
    assert_ne!(
        a,
        mac::sign(
            &SECRET,
            &HookMessage::Stop {
                harness: HarnessKind::CopilotCli
            }
        )
    );
}

#[test]
fn replies_are_checked_against_the_event() {
    let id = HookCallId::new();
    assert!(map_reply(HookReply::Allow { call_id: id }, Event::PreToolUse).is_ok());
    assert!(map_reply(HookReply::Allow { call_id: id }, Event::Stop).is_err());
    assert!(map_reply(HookReply::Allow { call_id: id }, Event::PostToolUse).is_err());
    assert!(map_reply(HookReply::Ack { call_id: id }, Event::PreToolUse).is_err());
    assert!(map_reply(HookReply::Ack { call_id: id }, Event::Stop).is_ok());
    assert!(map_reply(HookReply::Ack { call_id: id }, Event::PostToolUse).is_ok());
    let deny = map_reply(
        HookReply::Deny {
            call_id: id,
            reason: "r".into(),
        },
        Event::Stop,
    );
    assert_eq!(deny, Ok(crate::event::Outcome::Deny("r".into())));
}

#[test]
fn post_tool_use_and_stop_acks_print_nothing() {
    for (event, payload) in [
        (
            "post-tool-use",
            r#"{"tool_name":"Read","tool_response":{"is_error":false}}"#,
        ),
        ("stop", r#"{"hook_event_name":"Stop"}"#),
    ] {
        let relay = Relay::start(|s, r| send(s, reply_ack(r)));
        let env = good_env(&relay.endpoint);
        let out = run_hook(&hook_args("claude-code", event), &env, payload, &quick());
        relay.finish();
        assert_eq!((out.status, out.stdout.as_str()), (0, ""));
    }
}

#[test]
fn a_failed_post_tool_use_prints_nothing() {
    let env = good_env(&unused_endpoint("oh-none"));
    let out = run_hook(
        &hook_args("claude-code", "post-tool-use"),
        &env,
        r#"{"tool_name":"Read"}"#,
        &quick(),
    );
    assert_eq!((out.status, out.stdout.as_str()), (0, ""));
}

#[test]
fn bridge_environment_variants() {
    let good: std::collections::HashMap<String, String> = [
        ("ORDIN_MCP_ENDPOINT", "/run/mcp.sock".to_string()),
        ("ORDIN_SESSION_ID", RunId::new().to_string()),
        ("ORDIN_MCP_SERVER", "craft_notes".to_string()),
        ("ORDIN_MCP_TOKEN", b64url_encode(&[3u8; 32])),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let read = |m: &std::collections::HashMap<String, String>| {
        bridge::read_env(&|n: &str| m.get(n).cloned())
    };
    assert!(read(&good).is_ok());
    for (name, value) in [
        ("ORDIN_MCP_ENDPOINT", None),
        ("ORDIN_MCP_ENDPOINT", Some("rel")),
        ("ORDIN_SESSION_ID", None),
        ("ORDIN_SESSION_ID", Some("x")),
        ("ORDIN_MCP_SERVER", None),
        ("ORDIN_MCP_SERVER", Some("1abc")),
        ("ORDIN_MCP_SERVER", Some("")),
        ("ORDIN_MCP_TOKEN", None),
        ("ORDIN_MCP_TOKEN", Some("short")),
    ] {
        let mut m = good.clone();
        match value {
            Some(v) => m.insert(name.to_string(), v.to_string()),
            None => m.remove(name),
        };
        assert_eq!(
            read(&m).err().map(|f| f.code),
            Some("HOOK_BRIDGE_ENV_INVALID"),
            "{name}"
        );
    }
    let mut long = good.clone();
    long.insert("ORDIN_MCP_SERVER".into(), format!("a{}", "b".repeat(64)));
    assert!(read(&long).is_err());
}

#[test]
fn bridge_unreachable_core() {
    let env: std::collections::HashMap<String, String> = [
        ("ORDIN_MCP_ENDPOINT", unused_endpoint("om-none")),
        ("ORDIN_SESSION_ID", RunId::new().to_string()),
        ("ORDIN_MCP_SERVER", "work".to_string()),
        ("ORDIN_MCP_TOKEN", b64url_encode(&[3u8; 32])),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let out = run_hook(&args(&["mcp-bridge"]), &env, "", &quick());
    assert_eq!(out.status, 1);
    assert!(out.stderr.contains("HOOK_CORE_UNREACHABLE"));
}

#[test]
fn bridge_ends_when_the_connection_ends() {
    let listener = Listener::bind("om");
    let endpoint = listener.endpoint.clone();
    let front = std::thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let _ = crate::channel::read_line(&mut reader);
        stream.write_all(b"{\"kind\":\"ready\"}\nhello").unwrap();
    });
    let env: std::collections::HashMap<String, String> = [
        ("ORDIN_MCP_ENDPOINT", endpoint),
        ("ORDIN_SESSION_ID", RunId::new().to_string()),
        ("ORDIN_MCP_SERVER", "work".to_string()),
        ("ORDIN_MCP_TOKEN", b64url_encode(&[3u8; 32])),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    // Standard input never ends; the connection's end finishes the bridge.
    struct Never;
    impl std::io::Read for Never {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_secs(30));
            Ok(0)
        }
    }
    let out = run_with_input(&args(&["mcp-bridge"]), &env, Box::new(Never), &quick());
    front.join().unwrap();
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, "hello");
}

#[test]
fn key_file_checks() {
    let run =
        |root: &std::path::Path, id: RunId| api_key::run(id, Some(root), None, &mut Vec::new());
    let (root, id) = key_root(b"abc\r\n", 0o600);
    let mut out = Vec::new();
    api_key::run(id, Some(&root), None, &mut out).unwrap();
    assert_eq!(out, b"abc\n");
    let file = api_key::key_path(&root, id);
    // Unix only: H.2 checks the mode on macOS and Linux; Windows has no mode bits.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Stricter than 0600 is fine; any group or other bit is not.
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o400)).unwrap();
        assert!(run(&root, id).is_ok());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(run(&root, id).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(&file, vec![b'k'; 4097]).unwrap();
    assert!(run(&root, id).is_err());
    std::fs::write(&file, [0xffu8, 0xfe]).unwrap();
    assert!(run(&root, id).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn key_data_root_comes_from_the_endpoint() {
    let (root, id) = key_root(b"k\n", 0o600);
    let endpoint = root.join("run").join("hook.sock");
    let mut out = Vec::new();
    // H.1: macOS and Linux take the data root from the socket path; on Windows the endpoint is a
    // pipe name, so without `--data-root` the key is unreadable.
    let answer = api_key::run(id, None, Some(&endpoint.to_string_lossy()), &mut out);
    if cfg!(unix) {
        answer.unwrap();
        assert_eq!(out, b"k\n");
    } else {
        assert!(answer.is_err());
    }
    assert!(api_key::run(id, None, None, &mut Vec::new()).is_err());
    assert!(api_key::run(id, None, Some(r"\\.\pipe\ordin-hook-x"), &mut Vec::new()).is_err());
    assert!(api_key::run(id, None, Some("/"), &mut Vec::new()).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn key_output_failure_is_reported() {
    let (root, id) = key_root(b"k\n", 0o600);
    assert!(api_key::run(id, Some(&root), None, &mut Broken).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_secret_is_never_printed() {
    let relay = Relay::start(|s, r| send(s, reply_allow(r)));
    let env = good_env(&relay.endpoint);
    let out = run_hook(
        &hook_args("claude-code", "pre-tool-use"),
        &env,
        READ_PAYLOAD,
        &quick(),
    );
    relay.finish();
    let secret = env["ORDIN_HOOK_SECRET"].clone();
    assert!(!out.stdout.contains(&secret) && !out.stderr.contains(&secret));
}
