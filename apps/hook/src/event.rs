//! The program's own event and outcome types (OrdinHook.md *Rust modules inside apps/hook*).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    PreToolUse,
    PostToolUse,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Allow,
    Deny(String),
    Ack,
}

/// Longest tool name accepted (Ordin Hook D.1, D.3).
pub const TOOL_NAME_MAX: usize = 128;

/// `ToolOrigin.server`: 1-64 characters of `[a-zA-Z0-9_.-]` (2C).
pub fn server_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.chars().count())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// The server of a tool named `mcp__<server>__<tool>`, when it has that form.
pub fn mcp_server_of(tool: &str) -> Option<&str> {
    let rest = tool.strip_prefix("mcp__")?;
    let (server, tool_part) = rest.split_once("__")?;
    (!server.is_empty() && !tool_part.is_empty()).then_some(server)
}
