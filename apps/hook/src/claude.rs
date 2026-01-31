//! D.1 and the output forms for Claude Code (`PreToolUse`, `PostToolUse`, `Stop` command hooks).

use std::io::{self, Write};

use ordin_types::wire::hook::{HarnessKind, HookMessage};
use ordin_types::wire::rules::ToolOrigin;
use serde_json::{json, Value};

use crate::error::HookFailure;
use crate::event::{mcp_server_of, server_name_ok, Event, Outcome, TOOL_NAME_MAX};

pub fn read_event(stdin: &[u8], event: Event) -> Result<HookMessage, HookFailure> {
    let invalid = HookFailure::payload_invalid;
    let text = std::str::from_utf8(stdin).map_err(|_| invalid())?;
    let payload: Value = serde_json::from_str(text).map_err(|_| invalid())?;
    let object = payload.as_object().ok_or_else(invalid)?;
    let harness = HarnessKind::ClaudeCode;
    if event == Event::Stop {
        return Ok(HookMessage::Stop { harness });
    }
    let tool = object
        .get("tool_name")
        .and_then(Value::as_str)
        .filter(|t| (1..=TOOL_NAME_MAX).contains(&t.chars().count()))
        .ok_or_else(invalid)?
        .to_string();
    let harness_tool_use_id = object
        .get("tool_use_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    if event == Event::PostToolUse {
        let failed = object
            .get("tool_response")
            .and_then(|r| r.get("is_error"))
            .is_some_and(|v| v == &Value::Bool(true));
        return Ok(HookMessage::PostToolUse {
            harness,
            tool,
            harness_tool_use_id,
            succeeded: !failed,
        });
    }
    let arguments = object
        .get("tool_input")
        .filter(|a| a.is_object())
        .ok_or_else(invalid)?
        .clone();
    let named = object
        .get("mcp_server")
        .and_then(|s| s.get("name"))
        .and_then(Value::as_str);
    let origin = match named.or_else(|| mcp_server_of(&tool)) {
        Some(server) if server_name_ok(server) => ToolOrigin::PackageMcpServer {
            server: server.to_string(),
        },
        Some(_) => return Err(invalid()),
        None => ToolOrigin::BuiltIn,
    };
    Ok(HookMessage::PreToolUse {
        harness,
        tool,
        origin,
        arguments,
        harness_tool_use_id,
        // SPEC-GAP-2: the sub-agent field of the payload is unconfirmed (C-78); absent until the spike names it.
        subagent: None,
    })
}

pub fn write_decision(event: Event, outcome: &Outcome, out: &mut dyn Write) -> io::Result<()> {
    let line = match (event, outcome) {
        (Event::PreToolUse, Outcome::Allow) => Some(json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse", "permissionDecision": "allow"}})),
        (Event::PreToolUse, Outcome::Deny(reason)) => Some(json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse", "permissionDecision": "deny",
            "permissionDecisionReason": reason}})),
        (Event::Stop, Outcome::Deny(reason)) => {
            Some(json!({"decision": "block", "reason": reason}))
        }
        _ => None,
    };
    if let Some(value) = line {
        out.write_all(value.to_string().as_bytes())?;
        out.write_all(b"\n")?;
    }
    out.flush()
}
