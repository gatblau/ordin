//! The two short CLI checks: `claude --version` and `claude auth status --json`.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use tokio::process::Command;

/// `claude --version` prints `2.1.316 (Claude Code)`.
const VERSION_SUFFIX: &str = " (Claude Code)";

/// The child's environment, as `child_environment` built it.
pub(crate) type ChildEnv = [(String, OsString)];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VersionCheck {
    Found(String),
    NotFound,
    /// Timeout or unparsable output.
    Failed,
}

/// What `claude auth status --json` printed. `email` is never read (Security).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthStatus {
    pub logged_in: bool,
    pub exit_ok: bool,
    pub auth_method: Option<String>,
    pub api_provider: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AuthCheck {
    Read(AuthStatus),
    /// Timeout, no executable, or output that holds no `loggedIn`.
    Unreadable,
}

/// A `claude` child with no standard input and only the given environment.
pub(crate) fn claude_command(claude: &Path, env: &ChildEnv) -> Command {
    let mut command = Command::new(claude);
    command
        .env_clear()
        .envs(env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .kill_on_drop(true);
    command
}

async fn output(
    claude: &Path,
    env: &ChildEnv,
    args: &[&str],
    limit: Duration,
) -> Option<std::io::Result<Output>> {
    let mut command = claude_command(claude, env);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    tokio::time::timeout(limit, command.output()).await.ok()
}

fn parse_version(stdout: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stdout).ok()?.trim();
    let token = text.strip_suffix(VERSION_SUFFIX)?;
    (!token.is_empty() && !token.contains(char::is_whitespace)).then(|| token.to_owned())
}

// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA_VERSION_CHECK_FAILED
pub(crate) async fn check_version(claude: &Path, env: &ChildEnv, limit: Duration) -> VersionCheck {
    match output(claude, env, &["--version"], limit).await {
        None => VersionCheck::Failed,
        Some(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => VersionCheck::NotFound,
        Some(Err(_)) => VersionCheck::Failed,
        Some(Ok(done)) => {
            parse_version(&done.stdout).map_or(VersionCheck::Failed, VersionCheck::Found)
        }
    }
}

fn parse_auth(done: &Output) -> Option<AuthStatus> {
    let value: serde_json::Value = serde_json::from_slice(&done.stdout).ok()?;
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    Some(AuthStatus {
        logged_in: value.get("loggedIn")?.as_bool()?,
        exit_ok: done.status.success(),
        auth_method: text("authMethod"),
        api_provider: text("apiProvider"),
    })
}

pub(crate) async fn check_auth(claude: &Path, env: &ChildEnv, limit: Duration) -> AuthCheck {
    match output(claude, env, &["auth", "status", "--json"], limit).await {
        Some(Ok(done)) => parse_auth(&done).map_or(AuthCheck::Unreadable, AuthCheck::Read),
        _ => AuthCheck::Unreadable,
    }
}
