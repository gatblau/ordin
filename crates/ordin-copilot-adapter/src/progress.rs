//! The plain progress lines (CopilotLocalAdapter.md *Session events → run events*: the Claude Local
//! Adapter's table; never a tool identifier, 11:75).

use ordin_types::wire::rules::ToolOrigin;

/// Holding: the line shown while a tool request waits for a person (11:246).
pub(crate) const WAITING_FOR_APPROVAL: &str = "Waiting for your approval";

/// The Copilot CLI's built-in tools by what they do. The names are the CLI's own; the Copilot
/// compatibility proof (C-02, C-27) confirms them against the pinned CLI.
fn built_in_line(tool: &str) -> &'static str {
    match tool {
        "view" | "read" | "glob" | "grep" => "Read files",
        "edit" | "create" | "write" | "str_replace_editor" => "Changed a file",
        _ => "Used a tool",
    }
}

/// The Claude Local Adapter's server lines: MCP servers by server name.
fn server_line(server: &str) -> Option<&'static str> {
    match server {
        "sandbox" => Some("Ran code in the sandbox"),
        "memory" => Some("Checked what it remembers"),
        "mail" => Some("Worked with email"),
        "work" | "ordin" => None,
        "code" => Some("Looked at the code"),
        "telegram" => Some("Worked with Telegram"),
        s if s.starts_with("craft_") => Some("Worked on a file in an ArtCraft app"),
        _ => Some("Used a tool"),
    }
}

/// The line for one finished tool call; `None` where the table gives no line.
pub(crate) fn plain_line(tool: &str, origin: &ToolOrigin) -> Option<String> {
    let line = match origin {
        ToolOrigin::BuiltIn => Some(built_in_line(tool)),
        ToolOrigin::OrdinMcpFront { server } => server_line(server),
        ToolOrigin::PackageMcpServer { .. } => Some("Used one of the agent's own tools"),
    }?;
    Some(line.to_owned())
}
