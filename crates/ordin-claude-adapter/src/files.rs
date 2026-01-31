//! The per-session files (ClaudeLocalAdapter.md *Files written per session*): a `0700` directory
//! holding `0600` files, removed when the session ends for good.

use std::io::Write;
use std::path::{Path, PathBuf};

use ordin_mcp_front::{ServerName, SessionMcpConfig};
use ordin_types::{AgentInstanceId, RunId};
use serde_json::{json, Value};

use crate::config::PackageMcpServer;

/// The harness hook timeout ordin writes (LW-23, 2D-4), in seconds.
const HOOK_TIMEOUT_SECS: u32 = 60;

pub(crate) fn sessions_root(app_data_root: &Path) -> PathBuf {
    app_data_root.join("run").join("sessions")
}

pub(crate) fn session_dir(app_data_root: &Path, run_id: RunId) -> PathBuf {
    sessions_root(app_data_root).join(run_id.to_string())
}

/// Creates the directory, `0700`.
#[cfg(unix)]
pub(crate) fn create_session_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
pub(crate) fn create_session_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// Writes one session file, `0600`, replacing an earlier one.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.flush()
}

/// Removes every session directory left under `<root>/run/sessions/` by a crash (*Data Model*).
pub(crate) fn sweep_sessions(app_data_root: &Path) {
    let Ok(entries) = std::fs::read_dir(sessions_root(app_data_root)) else {
        return;
    };
    for entry in entries.flatten() {
        let _ = std::fs::remove_dir_all(entry.path());
    }
}

/// One word of a shell command line: the text itself when the shell would not change it, otherwise
/// quoted.
// The settings example shows the hook path unquoted. Claude Code gives `command` to a shell, so on
// 2.1.316 a path with a space fails and, with `onFailure: block`, every tool call is blocked. A path
// with no special character is written exactly as the example; any other is quoted.
pub(crate) fn shell_word(text: &str) -> String {
    shell_word_for(text, cfg!(windows))
}

/// [`shell_word`] for one platform. On Windows Claude Code runs a hook command through Git Bash,
/// which reads `\` as an escape: `C:\o\ordin-hook.exe` reaches it as `C:oordin-hook.exe`, and with
/// `onFailure: block` every tool call is refused. So a Windows path is written with `/`, which
/// Windows accepts, inside double quotes, with the characters Bash still reads there escaped.
pub(crate) fn shell_word_for(text: &str, windows: bool) -> String {
    if windows {
        let mut quoted = String::with_capacity(text.len() + 2);
        quoted.push('"');
        for c in text.chars() {
            match c {
                '\\' => quoted.push('/'),
                '"' | '$' | '`' => {
                    quoted.push('\\');
                    quoted.push(c);
                }
                _ => quoted.push(c),
            }
        }
        quoted.push('"');
        return quoted;
    }
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':'));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

fn hook_command(hook_program: &Path, event: &str, agent: AgentInstanceId) -> String {
    format!(
        "{} --harness claude-code --event {event} --agent-instance {agent}",
        shell_word(&hook_program.to_string_lossy())
    )
}

/// `settings.json`. `api_key_helper` is the `apiKeyHelper` line of `KeyRoute::SettingsApiKeyHelper`.
// spec: ordin-desktop/components/ClaudeLocalAdapter#SA-RECON-04
pub(crate) fn settings_json(
    hook_program: &Path,
    agent: AgentInstanceId,
    api_key_helper: Option<String>,
) -> Value {
    let pre = hook_command(hook_program, "pre-tool-use", agent);
    let post = hook_command(hook_program, "post-tool-use", agent);
    let stop = hook_command(hook_program, "stop", agent);
    let mut settings = json!({
        // A `!`-command in a skill's or command's text runs as shell before any tool call, so the
        // hook never sees it.
        "disableSkillShellExecution": true,
        "hooks": {
            "PreToolUse": [{"matcher": "*", "hooks": [{
                "type": "command", "command": pre, "timeout": HOOK_TIMEOUT_SECS, "onFailure": "block"
            }]}],
            "PostToolUse": [{"matcher": "*", "hooks": [{
                "type": "command", "command": post, "timeout": HOOK_TIMEOUT_SECS
            }]}],
            "Stop": [{"hooks": [{
                "type": "command", "command": stop, "timeout": HOOK_TIMEOUT_SECS
            }]}]
        }
    });
    if let Some(helper) = api_key_helper {
        settings["apiKeyHelper"] = Value::String(helper);
    }
    settings
}

/// `<hook_program> api-key --run <run_id> --data-root <app data root>` (OrdinHook.md `api-key` mode).
pub(crate) fn api_key_helper(hook_program: &Path, run_id: RunId, app_data_root: &Path) -> String {
    format!(
        "{} api-key --run {run_id} --data-root {}",
        shell_word(&hook_program.to_string_lossy()),
        shell_word(&app_data_root.to_string_lossy())
    )
}

/// What a session's `mcp.json` is made of: the file's text, and one launch file per package server
/// with the path to write it at.
pub(crate) struct McpFiles {
    pub mcp_json: Value,
    pub launches: Vec<(PathBuf, Vec<u8>)>,
}

/// `mcp.json`: the MCP Front's servers plus each package server under its own name. `Err` names a
/// package server whose name equals an MCP Front server's.
///
/// A package server starts through `ordin-hook mcp-launch`, and nothing of it but a path enters
/// `mcp.json`: a stdio server inherits Claude Code's whole environment, and Claude Code expands
/// `${NAME}` in `args` and `env` first, so a package could otherwise name the session's secret.
// spec: ordin-desktop/components/ClaudeLocalAdapter#SA-RECON-05
pub(crate) fn mcp_files(
    front: &SessionMcpConfig,
    package: &[PackageMcpServer],
    hook_program: &Path,
    session_dir: &Path,
) -> Result<McpFiles, String> {
    let front_names: Vec<&ServerName> = front.servers.iter().map(|s| &s.name).collect();
    let mut json = front.to_mcp_servers_json();
    let mut launches = Vec::new();
    for (index, server) in package.iter().enumerate() {
        if front_names.iter().any(|name| name.as_str() == server.name) {
            return Err(server.name.clone());
        }
        let path = session_dir.join(format!("package-server-{index}.json"));
        json["mcpServers"][&server.name] = json!({
            "type": "stdio",
            "command": hook_program.to_string_lossy(),
            "args": ["mcp-launch", "--spec", path.to_string_lossy()],
        });
        let launch = json!({
            "program": server.command.to_string_lossy(),
            "args": server.args,
            "env": server.env,
        });
        launches.push((path, launch.to_string().into_bytes()));
    }
    Ok(McpFiles {
        mcp_json: json,
        launches,
    })
}
