//! Child processes and the built-in Claude Code probe (SetupAssistant.md *Internal Logic* D.1, D.3).
//!
//! Every child is started without a shell, with fixed arguments and an environment built
//! explicitly from `PATH`, `HOME` (`USERPROFILE` on Windows) and `LANG` only, so no credential
//! variable reaches it (2D-1c).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use ordin_types::domain::identity::HarnessKind;
use tokio::process::Command;

use crate::types::{HarnessLocation, HarnessProbe, HarnessSignIn, ProbeError, ProbeRepair};

/// `--version`, `auth status`, a program's start check: 10 seconds.
pub(crate) const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
/// A kill by `SIGKILL` within this long of start with no output counts as refused (D.3.4).
const REFUSED_WITHIN: Duration = Duration::from_secs(2);
/// `SIGKILL` on macOS and Linux.
#[cfg(unix)]
const SIGKILL: i32 = 9;
/// Windows `ERROR_ACCESS_DISABLED_BY_POLICY` and `ERROR_SYSTEM_INTEGRITY_POLICY_VIOLATION`.
const WINDOWS_POLICY_ERRORS: [i32; 2] = [1260, 4551];

/// How a child ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ran {
    Exited {
        code: Option<i32>,
        stdout: String,
        stderr_empty: bool,
    },
    /// Refused by the operating system (D.3.4).
    Refused,
    NotFound,
    TimedOut,
    /// Any other spawn failure.
    Failed,
}

/// The D.1 environment, read from this process.
pub(crate) fn minimal_env(path: Option<&OsStr>) -> Vec<(&'static str, OsString)> {
    let mut env = Vec::new();
    match path {
        Some(p) => env.push(("PATH", p.to_owned())),
        None => {
            if let Some(p) = std::env::var_os("PATH") {
                env.push(("PATH", p));
            }
        }
    }
    let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    if let Some(h) = std::env::var_os(home) {
        env.push((home, h));
    }
    // spec: ordin-desktop/components/SetupAssistant#SET-R1-2
    // The account name, never a credential: Claude Code finds its sign-in in the macOS Keychain
    // under it, so without it a signed-in Claude Code reports `loggedIn: false`.
    let user = if cfg!(windows) { "USERNAME" } else { "USER" };
    if let Some(u) = std::env::var_os(user) {
        env.push((user, u));
    }
    if let Some(l) = std::env::var_os("LANG") {
        env.push(("LANG", l));
    }
    // spec: ordin-desktop/components/SetupAssistant#SET-R1-4
    // Windows' own folders, none of them a credential: without `SystemRoot` neither Node.js nor the
    // command interpreter starts, so `claude --version` ended with status 1 and Claude Code was never
    // found working (plan 04). The Claude adapter's sessions already carry the same names.
    #[cfg(windows)]
    for name in ["APPDATA", "LOCALAPPDATA", "SystemRoot", "TEMP"] {
        if let Some(value) = std::env::var_os(name) {
            env.push((name, value));
        }
    }
    env
}

/// Runs `program args` with the D.1 environment, stdin null, a timeout; never through a shell.
pub(crate) async fn run(
    program: &Path,
    args: &[String],
    path_env: Option<&OsStr>,
    timeout: Duration,
) -> Ran {
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .envs(minimal_env(path_env))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let started = Instant::now();
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ran::Refused,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ran::NotFound,
        Err(e)
            if e.raw_os_error()
                .is_some_and(|c| WINDOWS_POLICY_ERRORS.contains(&c)) =>
        {
            return Ran::Refused
        }
        Err(_) => return Ran::Failed,
    };
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(_)) => return Ran::Failed,
        Err(_) => return Ran::TimedOut,
    };
    let empty = output.stdout.is_empty() && output.stderr.is_empty();
    if killed(&output.status) && empty && started.elapsed() < REFUSED_WITHIN {
        return Ran::Refused;
    }
    Ran::Exited {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr_empty: output.stderr.is_empty(),
    }
}

#[cfg(unix)]
fn killed(status: &std::process::ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    status.signal() == Some(SIGKILL)
}

#[cfg(not(unix))]
fn killed(_status: &std::process::ExitStatus) -> bool {
    false
}

/// The first file named one of `names` in the `PATH` entries, in order.
pub(crate) fn find_on_path(path: Option<&OsStr>, names: &[&str]) -> Option<PathBuf> {
    let path = path?;
    for dir in std::env::split_paths(path) {
        for name in names {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The first whitespace-separated token matching `^[0-9]+\.[0-9]+\.[0-9]+` (D.1).
pub(crate) fn version_token(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let mut parts = token.splitn(3, '.');
        let major = parts.next()?;
        let minor = parts.next()?;
        let rest = parts.next()?;
        let patch: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        (digits(major) && digits(minor) && !patch.is_empty()).then(|| token.to_owned())
    })
}

/// Built in: Claude Code, per ADR-044.
pub struct ClaudeCodeProbe {
    /// The `PATH` searched and passed on; `None` reads this process's own at each call.
    path: Option<OsString>,
}

impl ClaudeCodeProbe {
    pub(crate) fn new() -> ClaudeCodeProbe {
        ClaudeCodeProbe { path: None }
    }

    #[cfg(test)]
    pub(crate) fn with_path(path: OsString) -> ClaudeCodeProbe {
        ClaudeCodeProbe { path: Some(path) }
    }

    fn path(&self) -> Option<OsString> {
        self.path.clone().or_else(|| std::env::var_os("PATH"))
    }

    fn program(&self) -> Option<PathBuf> {
        // SA-SETUP-03: `claude` (`claude.exe`, `claude.cmd` on Windows) on `PATH`.
        let names: &[&str] = if cfg!(windows) {
            &["claude.exe", "claude.cmd"]
        } else {
            &["claude"]
        };
        find_on_path(self.path().as_deref(), names)
    }

    async fn run(&self, args: &[&str]) -> Result<Ran, ProbeError> {
        let program = self.program().ok_or(ProbeError::NotFound)?;
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        Ok(run(&program, &args, self.path().as_deref(), COMMAND_TIMEOUT).await)
    }
}

fn probe_error(ran: &Ran) -> Option<ProbeError> {
    match ran {
        Ran::Refused => Some(ProbeError::Refused),
        Ran::NotFound => Some(ProbeError::NotFound),
        Ran::TimedOut => Some(ProbeError::Timeout),
        Ran::Failed => Some(ProbeError::Unreadable),
        Ran::Exited { .. } => None,
    }
}

#[async_trait::async_trait]
impl HarnessProbe for ClaudeCodeProbe {
    fn harness(&self) -> HarnessKind {
        HarnessKind::ClaudeCode
    }

    async fn locate(&self) -> Result<Option<HarnessLocation>, ProbeError> {
        let Some(program) = self.program() else {
            return Ok(None);
        };
        let ran = run(
            &program,
            &["--version".to_owned()],
            self.path().as_deref(),
            COMMAND_TIMEOUT,
        )
        .await;
        if let Some(error) = probe_error(&ran) {
            return Err(error);
        }
        let Ran::Exited { stdout, .. } = ran else {
            return Err(ProbeError::Unreadable);
        };
        let version = version_token(&stdout).ok_or(ProbeError::Unreadable)?;
        Ok(Some(HarnessLocation {
            path: program,
            version,
        }))
    }

    async fn sign_in_status(&self) -> Result<HarnessSignIn, ProbeError> {
        let ran = self.run(&["auth", "status", "--json"]).await?;
        if let Some(error) = probe_error(&ran) {
            return Err(error);
        }
        let Ran::Exited { code, stdout, .. } = ran else {
            return Err(ProbeError::Unreadable);
        };
        if code != Some(0) {
            return Ok(HarnessSignIn::SignedOut);
        }
        let Ok(status) = serde_json::from_str::<serde_json::Value>(stdout.trim()) else {
            return Err(ProbeError::Unreadable);
        };
        if status.get("loggedIn").and_then(serde_json::Value::as_bool) == Some(true) {
            // spec: ordin-desktop/components/SetupAssistant#SA-SETUP-04
            let method = status
                .get("authMethod")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            Ok(HarnessSignIn::SignedIn { method })
        } else {
            Ok(HarnessSignIn::SignedOut)
        }
    }

    async fn start_sign_in(&self) -> Result<(), ProbeError> {
        let program = self.program().ok_or(ProbeError::NotFound)?;
        let mut command = Command::new(program);
        command
            .args(["auth", "login"])
            .env_clear()
            .envs(minimal_env(self.path().as_deref()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match command.status().await {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err(ProbeError::Refused),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ProbeError::NotFound),
            Err(_) => Err(ProbeError::Unreadable),
        }
    }

    async fn repair(&self) -> Result<ProbeRepair, ProbeError> {
        Ok(ProbeRepair::NothingToDo)
    }
}
