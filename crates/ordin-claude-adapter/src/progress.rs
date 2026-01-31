//! The plain progress lines (ClaudeLocalAdapter.md *Hook Relay session events → run events*; never
//! a tool identifier, 11:75).

use ordin_types::wire::rules::ToolOrigin;

/// Holding: the line shown while the hook is held (11:246).
pub(crate) const WAITING_FOR_APPROVAL: &str = "Waiting for your approval";
const DID_NOT_WORK: &str = " (it didn't work)";

fn built_in_line(tool: &str) -> Option<&'static str> {
    match tool {
        "Read" | "Glob" | "Grep" => Some("Read files"),
        "Edit" | "Write" => Some("Changed a file"),
        "Bash" => Some("Ran an approved command"),
        "TodoWrite" => None,
        // At Claude Code 2.1.316 the hook names this tool `Agent`, while `--tools` and the init event
        // say `Task`.
        "Task" | "Agent" => Some("Asked a helper"),
        _ => Some("Used a tool"),
    }
}

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
pub(crate) fn plain_line(tool: &str, origin: &ToolOrigin, succeeded: bool) -> Option<String> {
    let line = match origin {
        ToolOrigin::BuiltIn => built_in_line(tool),
        ToolOrigin::OrdinMcpFront { server } => server_line(server),
        ToolOrigin::PackageMcpServer { .. } => Some("Used one of the agent's own tools"),
    }?;
    Some(if succeeded {
        line.to_owned()
    } else {
        format!("{line}{DID_NOT_WORK}")
    })
}
