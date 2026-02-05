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

/// The folders searched for `claude` after `PATH` (SET-R1-7): the person's own `.local/bin` (home
/// folder from `HOME`, or `USERPROFILE` on Windows) and, on macOS, the two Homebrew folders. A program
/// started from Finder, the Dock or the Start menu gets only the system's short `PATH`, which leaves
/// these out. The desktop program finds the Claude adapter's `claude` with the same list.
// spec: ordin-desktop/components/SetupAssistant#SET-R1-7
pub(crate) fn claude_code_folders(home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = home
        .into_iter()
        .map(|home| home.join(".local").join("bin"))
        .collect();
    if cfg!(target_os = "macos") {
        folders.push(PathBuf::from("/opt/homebrew/bin"));
        folders.push(PathBuf::from("/usr/local/bin"));
    }
    folders
}

/// The person's home folder from this process's environment (`USERPROFILE` on Windows).
fn home_folder() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// `claude` (`claude.exe`, `claude.cmd` on Windows) in the `PATH` entries, then in `extra`, in order.
pub(crate) fn find_claude_in(path: Option<&OsStr>, extra: &[PathBuf]) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    find_on_path(path, names).or_else(|| {
        extra
            .iter()
            .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
            .find(|candidate| is_executable_file(candidate))
    })
}

/// The Claude Code program the probe uses: `claude` on `path`, else in the person's own folders of
/// SET-R1-7 under `home`; `None` when it is in neither. The desktop program passes this process's
/// `PATH` and home folder, and gives the answer to the Claude Local Adapter (OQ-REC-4 option (b)).
// spec: ordin-desktop/components/SetupAssistant#SET-R1-7
pub fn find_claude_code(path: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    find_claude_in(path, &claude_code_folders(home))
}

/// The folders searched for `copilot` after `PATH` (SET-CPX-2), in order: `<home>/.local/bin` (the
/// install script), on macOS `/opt/homebrew/bin` and `/usr/local/bin` (Homebrew), on Windows
/// `<LOCALAPPDATA>\Microsoft\WinGet\Links` (WinGet) and `<APPDATA>\npm` (npm), then off Windows the
/// Node version manager's folders (SET-CPX-7). An absent variable skips its folder. Conditional on C-112.
// spec: ordin-desktop/components/SetupAssistant#SET-CPX-2
pub(crate) fn copilot_cli_folders(
    home: Option<&Path>,
    local_app_data: Option<&Path>,
    app_data: Option<&Path>,
) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = home
        .into_iter()
        .map(|home| home.join(".local").join("bin"))
        .collect();
    if cfg!(target_os = "macos") {
        folders.push(PathBuf::from("/opt/homebrew/bin"));
        folders.push(PathBuf::from("/usr/local/bin"));
    }
    if cfg!(windows) {
        folders.extend(local_app_data.map(|d| d.join("Microsoft").join("WinGet").join("Links")));
        folders.extend(app_data.map(|d| d.join("npm")));
    }
    if !cfg!(windows) {
        if let Some(home) = home {
            folders.extend(nvm_bin_folders(&home.join(".nvm").join("versions")));
        }
    }
    folders
}

/// SET-CPX-7: the `bin` folder of each Node.js version a Node version manager installed under
/// `versions` (`versions/node/<v>/bin`, then `versions/<v>/bin`), newest version first. A program
/// opened from the Dock or a desktop menu has none of the shell's version-manager `PATH`, so an npm
/// install of `copilot` is found only here; `node` sits in the same folder, which SET-CPX-3 adds to
/// `PATH`. A missing or unreadable folder gives none.
// spec: ordin-desktop/components/SetupAssistant#SET-CPX-7
pub(crate) fn nvm_bin_folders(versions: &Path) -> Vec<PathBuf> {
    let mut found: Vec<(Vec<u64>, PathBuf)> = Vec::new();
    for parent in [versions.join("node"), versions.to_path_buf()] {
        let Ok(entries) = std::fs::read_dir(&parent) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(key) = node_version_key(&name.to_string_lossy()) else {
                continue;
            };
            let bin = entry.path().join("bin");
            if bin.is_dir() {
                found.push((key, bin));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, bin)| bin).collect()
}

/// `v24.16.0` → `[24, 16, 0]`; a name that is not a version → `None`.
fn node_version_key(name: &str) -> Option<Vec<u64>> {
    let digits = name.strip_prefix('v')?;
    let key: Option<Vec<u64>> = digits.split('.').map(|p| p.parse().ok()).collect();
    key.filter(|k| !k.is_empty())
}

/// `copilot` (`copilot.exe`, then `copilot.cmd`, on Windows) in the `PATH` entries, then in `extra`.
pub(crate) fn find_copilot_in(path: Option<&OsStr>, extra: &[PathBuf]) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["copilot.exe", "copilot.cmd"]
    } else {
        &["copilot"]
    };
    find_on_path(path, names).or_else(|| {
        extra
            .iter()
            .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
            .find(|candidate| is_executable_file(candidate))
    })
}

/// The Copilot CLI the person installed: `copilot` on `path`, else in the person's own folders of
/// SET-CPX-2; `None` when it is in neither. Kept beside [`find_claude_code`] so the desktop's probe and
/// the adapter's CLI source never disagree (ADR-123).
// spec: ordin-desktop/components/SetupAssistant#SET-CPX-2
pub fn find_copilot_cli(
    path: Option<&OsStr>,
    home: Option<&Path>,
    local_app_data: Option<&Path>,
    app_data: Option<&Path>,
) -> Option<PathBuf> {
    find_copilot_in(path, &copilot_cli_folders(home, local_app_data, app_data))
}

/// `path` with `folder` added at the end when no entry is already that folder (SET-R1-8), so what
/// `claude` itself starts from its own folder is found. A folder that cannot be joined into a `PATH`
/// leaves `path` as it was.
// spec: ordin-desktop/components/SetupAssistant#SET-R1-8
pub(crate) fn path_with_folder(path: Option<OsString>, folder: Option<&Path>) -> Option<OsString> {
    let Some(folder) = folder.filter(|f| !f.as_os_str().is_empty()) else {
        return path;
    };
    let mut entries: Vec<PathBuf> = path
        .as_deref()
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    if entries.iter().any(|entry| entry == folder) {
        return path;
    }
    entries.push(folder.to_path_buf());
    std::env::join_paths(entries).ok().or(path)
}

/// Built in: Claude Code, per ADR-044.
pub struct ClaudeCodeProbe {
    /// The `PATH` searched and passed on; `None` reads this process's own at each call.
    path: Option<OsString>,
    /// The folders searched after `PATH` (SET-R1-7); `None` reads the home folder at each call.
    extra: Option<Vec<PathBuf>>,
}

impl ClaudeCodeProbe {
    pub(crate) fn new() -> ClaudeCodeProbe {
        ClaudeCodeProbe {
            path: None,
            extra: None,
        }
    }

    /// Only `path` is searched: no folder of the test runner's own home is.
    #[cfg(test)]
    pub(crate) fn with_path(path: OsString) -> ClaudeCodeProbe {
        ClaudeCodeProbe {
            path: Some(path),
            extra: Some(Vec::new()),
        }
    }

    /// `path`, then the SET-R1-7 folders under `home`.
    #[cfg(all(test, unix))]
    pub(crate) fn with_path_and_home(path: OsString, home: &Path) -> ClaudeCodeProbe {
        ClaudeCodeProbe {
            path: Some(path),
            extra: Some(claude_code_folders(Some(home))),
        }
    }

    fn path(&self) -> Option<OsString> {
        self.path.clone().or_else(|| std::env::var_os("PATH"))
    }

    fn program(&self) -> Option<PathBuf> {
        // SA-SETUP-03, SET-R1-7: `claude` on `PATH`, then in the person's own folders.
        let extra = self
            .extra
            .clone()
            .unwrap_or_else(|| claude_code_folders(home_folder().as_deref()));
        find_claude_in(self.path().as_deref(), &extra)
    }

    /// The `PATH` a command of `program` gets: this one, with `program`'s folder added (SET-R1-8).
    fn child_path(&self, program: &Path) -> Option<OsString> {
        path_with_folder(self.path(), program.parent())
    }

    async fn run(&self, args: &[&str]) -> Result<Ran, ProbeError> {
        let program = self.program().ok_or(ProbeError::NotFound)?;
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let path = self.child_path(&program);
        Ok(run(&program, &args, path.as_deref(), COMMAND_TIMEOUT).await)
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
        let path = self.child_path(&program);
        let ran = run(
            &program,
            &["--version".to_owned()],
            path.as_deref(),
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
        let path = self.child_path(&program);
        let mut command = Command::new(program);
        command
            .args(["auth", "login"])
            .env_clear()
            .envs(minimal_env(path.as_deref()))
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
