//! D.2 and the output forms for the Copilot CLI (`preToolUse`, `postToolUse`). Accepts the Claude-compatible
//! payload too (C-27).

use std::io::{self, Write};

use ordin_types::wire::hook::{HarnessKind, HookMessage};
use ordin_types::wire::rules::ToolOrigin;
use serde_json::{json, Value};

use crate::error::HookFailure;
use crate::event::{mcp_server_of, server_name_ok, Event, Outcome, TOOL_NAME_MAX};

/// The Copilot CLI's built-in tools (Ordin Hook D.2).
pub const BUILT_IN_TOOLS: [&str; 6] = [
    "view",
    "create",
    "edit",
    "bash",
    "powershell",
    "report_intent",
];

pub fn read_event(stdin: &[u8], event: Event) -> Result<HookMessage, HookFailure> {
    let invalid = HookFailure::payload_invalid;
    let text = std::str::from_utf8(stdin).map_err(|_| invalid())?;
    let payload: Value = serde_json::from_str(text).map_err(|_| invalid())?;
    let object = payload.as_object().ok_or_else(invalid)?;
    let harness = HarnessKind::CopilotCli;
    if event == Event::Stop {
        return Ok(HookMessage::Stop { harness });
    }
    let tool = object
        .get("toolName")
        .or_else(|| object.get("tool_name"))
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
            .get("toolResult")
            .and_then(|r| r.get("resultType"))
            .and_then(Value::as_str)
            == Some("failure");
        return Ok(HookMessage::PostToolUse {
            harness,
            tool,
            harness_tool_use_id,
            succeeded: !failed,
        });
    }
    let raw_args = object
        .get("toolArgs")
        .or_else(|| object.get("tool_input"))
        .ok_or_else(invalid)?;
    let arguments = match raw_args {
        Value::String(text) => serde_json::from_str::<Value>(text).map_err(|_| invalid())?,
        other => other.clone(),
    };
    if !arguments.is_object() {
        return Err(invalid());
    }
    let origin = if let Some(server) = mcp_server_of(&tool) {
        if !server_name_ok(server) {
            return Err(invalid());
        }
        ToolOrigin::PackageMcpServer {
            server: server.to_string(),
        }
    } else if BUILT_IN_TOOLS.contains(&tool.as_str()) {
        ToolOrigin::BuiltIn
    } else {
        // SA-HOOK-04: an unrecognised tool is never claimed as built-in.
        ToolOrigin::PackageMcpServer {
            server: "unknown".to_string(),
        }
    };
    Ok(HookMessage::PreToolUse {
        harness,
        tool,
        origin,
        arguments,
        harness_tool_use_id,
        // SPEC-GAP-2: see the Claude Code mapping.
        subagent: None,
    })
}

pub fn write_decision(event: Event, outcome: &Outcome, out: &mut dyn Write) -> io::Result<()> {
    let line = match (event, outcome) {
        (Event::PreToolUse, Outcome::Allow) => Some(json!({"permissionDecision": "allow"})),
        (Event::PreToolUse, Outcome::Deny(reason)) => Some(json!({
            "permissionDecision": "deny", "permissionDecisionReason": reason})),
        _ => None,
    };
    if let Some(value) = line {
        out.write_all(value.to_string().as_bytes())?;
        out.write_all(b"\n")?;
    }
    out.flush()
}
