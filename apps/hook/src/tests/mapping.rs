//! C-27: conformance tests of the Copilot payload mapping (D.2), and the Claude Code mapping (D.1).

use ordin_types::wire::hook::{HarnessKind, HookMessage};
use ordin_types::wire::rules::ToolOrigin;
use serde_json::json;

use crate::event::{Event, Outcome};
use crate::{claude, copilot};

fn pre(harness_copilot: bool, payload: serde_json::Value) -> Result<HookMessage, &'static str> {
    let bytes = payload.to_string();
    let read = if harness_copilot {
        copilot::read_event(bytes.as_bytes(), Event::PreToolUse)
    } else {
        claude::read_event(bytes.as_bytes(), Event::PreToolUse)
    };
    read.map_err(|f| f.code)
}

fn parts(m: HookMessage) -> (String, ToolOrigin, serde_json::Value, Option<String>) {
    match m {
        HookMessage::PreToolUse {
            tool,
            origin,
            arguments,
            harness_tool_use_id,
            subagent,
            ..
        } => {
            assert!(subagent.is_none(), "C-78: the sub-agent is not read yet");
            (tool, origin, arguments, harness_tool_use_id)
        }
        other => panic!("{other:?}"),
    }
}

fn written(f: impl FnOnce(&mut Vec<u8>) -> std::io::Result<()>) -> String {
    let mut out = Vec::new();
    f(&mut out).unwrap();
    String::from_utf8(out).unwrap()
}

// ---- Copilot CLI (C-27) ----

#[test]
fn copilot_string_arguments_are_parsed_and_objects_kept() {
    let (_, _, a, _) = parts(
        pre(
            true,
            json!({"toolName": "bash", "toolArgs": "{\"command\":\"ls\"}"}),
        )
        .unwrap(),
    );
    assert_eq!(a, json!({"command": "ls"}));
    let (_, _, a, _) = parts(
        pre(
            true,
            json!({"toolName": "bash", "toolArgs": {"command": "ls"}}),
        )
        .unwrap(),
    );
    assert_eq!(a, json!({"command": "ls"}));
}

#[test]
fn copilot_accepts_the_claude_compatible_form() {
    let (tool, origin, args, id) = parts(
        pre(
            true,
            json!({"tool_name": "view", "tool_input": {"path": "a"}, "tool_use_id": "t1"}),
        )
        .unwrap(),
    );
    assert_eq!(tool, "view");
    assert!(matches!(origin, ToolOrigin::BuiltIn));
    assert_eq!(args, json!({"path": "a"}));
    assert_eq!(id.as_deref(), Some("t1"));
}

#[test]
fn copilot_built_in_list_is_built_in() {
    for name in [
        "view",
        "create",
        "edit",
        "bash",
        "powershell",
        "report_intent",
    ] {
        let (_, origin, _, _) =
            parts(pre(true, json!({"toolName": name, "toolArgs": {}})).unwrap());
        assert!(matches!(origin, ToolOrigin::BuiltIn), "{name}");
    }
}

#[test]
fn copilot_mcp_names_carry_their_server() {
    let (_, origin, _, _) = parts(
        pre(
            true,
            json!({"toolName": "mcp__supplier-db__update_rate", "toolArgs": {}}),
        )
        .unwrap(),
    );
    assert!(matches!(origin, ToolOrigin::PackageMcpServer { server } if server == "supplier-db"));
}

#[test]
fn copilot_unknown_tool_is_never_built_in() {
    let (_, origin, _, _) = parts(pre(true, json!({"toolName": "grep", "toolArgs": {}})).unwrap());
    assert!(matches!(origin, ToolOrigin::PackageMcpServer { server } if server == "unknown"));
}

#[test]
fn copilot_rejects_unreadable_payloads() {
    let bad = [
        json!({"toolArgs": {}}),
        json!({"toolName": "bash"}),
        json!({"toolName": "bash", "toolArgs": "not json"}),
        json!({"toolName": "bash", "toolArgs": "[1]"}),
        json!({"toolName": "bash", "toolArgs": 5}),
        json!({"toolName": "mcp__bad server__t", "toolArgs": {}}),
        json!({"toolName": "", "toolArgs": {}}),
    ];
    for payload in bad {
        assert_eq!(
            pre(true, payload.clone()).err(),
            Some("HOOK_PAYLOAD_INVALID"),
            "{payload}"
        );
    }
    assert!(copilot::read_event(&[0xff, 0xfe], Event::PreToolUse).is_err());
    assert!(copilot::read_event(b"[]", Event::PreToolUse).is_err());
}

#[test]
fn copilot_post_tool_use_reads_the_result_type() {
    for (result, expected) in [("success", true), ("failure", false), ("denied", true)] {
        let bytes = json!({"toolName": "bash", "toolResult": {"resultType": result}}).to_string();
        match copilot::read_event(bytes.as_bytes(), Event::PostToolUse).unwrap() {
            HookMessage::PostToolUse {
                harness,
                succeeded,
                tool,
                ..
            } => {
                assert_eq!(harness, HarnessKind::CopilotCli);
                assert_eq!(tool, "bash");
                assert_eq!(succeeded, expected, "{result}");
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(copilot::read_event(b"{}", Event::PostToolUse).is_err());
}

#[test]
fn copilot_stop_reads_no_fields() {
    let m = copilot::read_event(b"{}", Event::Stop).unwrap();
    assert!(matches!(
        m,
        HookMessage::Stop {
            harness: HarnessKind::CopilotCli
        }
    ));
}

#[test]
fn copilot_output_forms() {
    let allow = written(|o| copilot::write_decision(Event::PreToolUse, &Outcome::Allow, o));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&allow).unwrap(),
        json!({"permissionDecision": "allow"})
    );
    let deny =
        written(|o| copilot::write_decision(Event::PreToolUse, &Outcome::Deny("no".into()), o));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&deny).unwrap(),
        json!({"permissionDecision": "deny", "permissionDecisionReason": "no"})
    );
    assert_eq!(
        written(|o| copilot::write_decision(Event::PostToolUse, &Outcome::Ack, o)),
        ""
    );
    assert_eq!(
        written(|o| copilot::write_decision(Event::Stop, &Outcome::Deny("x".into()), o)),
        ""
    );
}

// ---- Claude Code (D.1) ----

#[test]
fn claude_mcp_server_field_names_the_server() {
    let (tool, origin, args, id) = parts(
        pre(
            false,
            json!({"tool_name": "mcp__supplier-db__update_rate", "tool_input": {"rate": 412.5},
            "tool_use_id": "toolu_01", "mcp_server": {"name": "supplier-db", "source": "dynamic"}}),
        )
        .unwrap(),
    );
    assert_eq!(tool, "mcp__supplier-db__update_rate");
    assert!(matches!(origin, ToolOrigin::PackageMcpServer { server } if server == "supplier-db"));
    assert_eq!(args, json!({"rate": 412.5}));
    assert_eq!(id.as_deref(), Some("toolu_01"));
}

#[test]
fn claude_mcp_tool_name_form_and_built_in() {
    let (_, origin, _, _) = parts(
        pre(
            false,
            json!({"tool_name": "mcp__work__list", "tool_input": {}}),
        )
        .unwrap(),
    );
    assert!(matches!(origin, ToolOrigin::PackageMcpServer { server } if server == "work"));
    for name in ["Read", "mcp__only", "mcp____x"] {
        let (_, origin, _, id) =
            parts(pre(false, json!({"tool_name": name, "tool_input": {}})).unwrap());
        assert!(matches!(origin, ToolOrigin::BuiltIn), "{name}");
        assert!(id.is_none());
    }
}

#[test]
fn claude_rejects_unreadable_payloads() {
    let bad = [
        json!({"tool_input": {}}),
        json!({"tool_name": "Read"}),
        json!({"tool_name": "Read", "tool_input": []}),
        json!({"tool_name": "x".repeat(129), "tool_input": {}}),
        json!({"tool_name": "Read", "tool_input": {}, "mcp_server": {"name": "bad name"}}),
        json!({"tool_name": "Read", "tool_input": {}, "mcp_server": {"name": "a".repeat(65)}}),
    ];
    for payload in bad {
        assert_eq!(
            pre(false, payload.clone()).err(),
            Some("HOOK_PAYLOAD_INVALID"),
            "{payload}"
        );
    }
    assert!(pre(
        false,
        json!({"tool_name": "x".repeat(128), "tool_input": {}})
    )
    .is_ok());
    assert!(claude::read_event(&[0xff], Event::PreToolUse).is_err());
    assert!(claude::read_event(b"nope", Event::Stop).is_err());
}

#[test]
fn claude_post_tool_use_reads_is_error() {
    for (response, expected) in [
        (json!({"is_error": true}), false),
        (json!({"is_error": false}), true),
        (json!("text"), true),
        (json!(null), true),
    ] {
        let bytes =
            json!({"tool_name": "Bash", "tool_use_id": "t", "tool_response": response}).to_string();
        match claude::read_event(bytes.as_bytes(), Event::PostToolUse).unwrap() {
            HookMessage::PostToolUse {
                succeeded,
                harness_tool_use_id,
                ..
            } => {
                assert_eq!(succeeded, expected);
                assert_eq!(harness_tool_use_id.as_deref(), Some("t"));
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(claude::read_event(b"{}", Event::PostToolUse).is_err());
}

#[test]
fn claude_output_forms() {
    let value = |s: String| serde_json::from_str::<serde_json::Value>(&s).unwrap();
    assert_eq!(
        value(written(|o| claude::write_decision(
            Event::PreToolUse,
            &Outcome::Allow,
            o
        ))),
        json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow"}})
    );
    assert_eq!(
        value(written(|o| claude::write_decision(
            Event::PreToolUse,
            &Outcome::Deny("r".into()),
            o
        ))),
        json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "r"}})
    );
    assert_eq!(
        value(written(|o| claude::write_decision(
            Event::Stop,
            &Outcome::Deny("r".into()),
            o
        ))),
        json!({"decision": "block", "reason": "r"})
    );
    assert_eq!(
        written(|o| claude::write_decision(Event::Stop, &Outcome::Ack, o)),
        ""
    );
    assert_eq!(
        written(|o| claude::write_decision(Event::PostToolUse, &Outcome::Ack, o)),
        ""
    );
}
