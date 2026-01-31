//! The Claude Code command line (ClaudeLocalAdapter.md *Command line*). `--bare` is never passed
//! (09 ADR-044).

use std::ffi::OsString;
use std::path::Path;

use crate::config::SessionSetup;

/// The built-in tools of a desktop or personal-server session, checked by the init-event conformance
/// test at the pinned Claude Code version.
// spec: ordin-desktop/components/ClaudeLocalAdapter#DESKTOP_BUILT_IN_TOOLS
pub const DESKTOP_BUILT_IN_TOOLS: &[&str] = &["Read", "Edit", "Write", "Glob", "Grep", "Task"];

/// The agent profile's tools: reading only (02:508), conditional on C-79.
// spec: ordin-desktop/components/ClaudeLocalAdapter#C-79
const AGENT_PROFILE_TOOLS: &[&str] = &["Read"];
const AGENT_PROFILE_DISALLOWED: &str = "Write,Edit";

/// How the session's id is passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionFlag {
    New,
    Resume,
}

/// What the command line needs beyond the session setup.
pub(crate) struct CommandInput<'a> {
    pub runtime_session_id: &'a str,
    pub flag: SessionFlag,
    pub session_dir: &'a Path,
    pub setup: &'a SessionSetup,
    pub agent_profile: bool,
    /// The Rules Engine's narrowing of `--tools` at run start.
    pub tool_subset: Option<&'a [String]>,
}

fn tool_list(input: &CommandInput<'_>) -> String {
    let mut tools: Vec<&str> = if input.agent_profile {
        AGENT_PROFILE_TOOLS.to_vec()
    } else {
        let mut tools = DESKTOP_BUILT_IN_TOOLS.to_vec();
        if input.setup.shell_allowed {
            tools.push("Bash");
        }
        tools
    };
    if let Some(subset) = input.tool_subset {
        tools.retain(|tool| subset.iter().any(|allowed| allowed == tool));
    }
    tools.join(",")
}

fn flag(args: &mut Vec<OsString>, name: &str, value: impl Into<OsString>) {
    args.push(name.into());
    args.push(value.into());
}

/// The arguments after the program name, in the order of *Command line*.
// spec: ordin-desktop/components/ClaudeLocalAdapter#C-26
// spec: ordin-desktop/components/ClaudeLocalAdapter#C-61
pub(crate) fn session_args(input: &CommandInput<'_>) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["-p".into()];
    flag(&mut args, "--input-format", "stream-json");
    flag(&mut args, "--output-format", "stream-json");
    args.push("--verbose".into());
    match input.flag {
        SessionFlag::New => flag(&mut args, "--session-id", input.runtime_session_id),
        SessionFlag::Resume => flag(&mut args, "--resume", input.runtime_session_id),
    }
    args.push("--restricted".into());
    args.push("--strict-mcp-config".into());
    flag(
        &mut args,
        "--mcp-config",
        input.session_dir.join("mcp.json"),
    );
    flag(
        &mut args,
        "--settings",
        input.session_dir.join("settings.json"),
    );
    flag(&mut args, "--permission-prompts", "none");
    let setup = input.setup;
    if setup.append_system_prompt_file.is_file() {
        flag(
            &mut args,
            "--append-system-prompt-file",
            &setup.append_system_prompt_file,
        );
    }
    if setup.plugin_dir.is_dir() {
        flag(&mut args, "--plugin-dir", &setup.plugin_dir);
    }
    flag(&mut args, "--tools", tool_list(input));
    if input.agent_profile {
        flag(&mut args, "--disallowedTools", AGENT_PROFILE_DISALLOWED);
    }
    for dir in &setup.extra_dirs {
        flag(&mut args, "--add-dir", dir);
    }
    if let Some(model) = &setup.model {
        flag(&mut args, "--model", model);
    }
    if let Some(effort) = setup.effort {
        flag(&mut args, "--effort", effort.flag_value());
    }
    if input.agent_profile {
        if let Some(fallback) = &setup.fallback_model {
            flag(&mut args, "--fallback-model", fallback);
        }
    }
    args
}

/// The stream-json `user` line for one turn (fields in the spec's order, newline ended).
pub(crate) fn user_line(text: &str) -> Vec<u8> {
    #[derive(serde::Serialize)]
    struct Message<'a> {
        role: &'static str,
        content: &'a str,
    }
    #[derive(serde::Serialize)]
    struct Line<'a> {
        r#type: &'static str,
        message: Message<'a>,
    }
    let line = Line {
        r#type: "user",
        message: Message {
            role: "user",
            content: text,
        },
    };
    // Serialising two strings cannot fail.
    let mut bytes = serde_json::to_vec(&line).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}
