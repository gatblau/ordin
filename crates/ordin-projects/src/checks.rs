//! Check commands (ADR-021, ADR-053; Internal Logic 7).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ordin_rules::{ApprovalWait, CommandOrigin, CommandRequest};
use ordin_types::wire::rules::Verdict;
use ordin_types::{CodingTaskId, ProjectId};
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::error::ProjectError;
use crate::service::{lock, ProjectService};
use crate::types::ProjectCheckResult;

/// The only variables a check receives. A check is project code an agent may have written, so a
/// token, key or agent socket in the person's environment stays out of it (ADR-021).
// spec: ordin-desktop/components/ProjectService#SA-RECON-12
pub(crate) const CHECK_ENVIRONMENT: [&str; 49] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "TMPDIR",
    "TMP",
    "TEMP",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    // Toolchains
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "GOROOT",
    "GOCACHE",
    "GOMODCACHE",
    "JAVA_HOME",
    "GRADLE_USER_HOME",
    "NVM_DIR",
    "VIRTUAL_ENV",
    "CONDA_PREFIX",
    // Windows
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "CommonProgramFiles",
    "OS",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    // Proxies, as the `gh` child has them
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
];

/// How long a finished command's output pipes may stay open (a background child can hold them).
const OUTPUT_DRAIN: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn capture<R: AsyncRead + Unpin>(mut from: R, into: Arc<Mutex<Captured>>) {
    let mut buffer = [0u8; 8192];
    while let Ok(read) = from.read(&mut buffer).await {
        if read == 0 {
            return;
        }
        let mut captured = lock(&into);
        let room = crate::CHECK_OUTPUT_MAX.saturating_sub(captured.bytes.len());
        let kept = read.min(room);
        captured.bytes.extend_from_slice(&buffer[..kept]);
        if kept < read {
            captured.truncated = true;
        }
    }
}

/// The platform shell's arguments, as a list.
fn shell_arguments(shell: &mut Command, command: &str) {
    if cfg!(windows) {
        shell.args(["/d", "/s", "/c", command]);
        // cmd looks in the working folder first, so `gh` there would be a program an agent wrote.
        shell.env("NoDefaultCurrentDirectoryInExePath", "1");
    } else {
        shell.args(["-c", command]);
    }
}

/// Kills the command and everything it started.
async fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    let mut kill = if cfg!(windows) {
        let mut kill = Command::new("taskkill");
        kill.args(["/T", "/F", "/PID", &pid.to_string()]);
        kill
    } else {
        let mut kill = Command::new("kill");
        kill.args(["-KILL", "--", &format!("-{pid}")]);
        kill
    };
    kill.stdout(Stdio::null()).stderr(Stdio::null());
    let _ = kill.status().await;
}

/// A proxy value without its `user:password@`: a check or server has no use for them, and they are
/// a credential. None for a value that is not text, which cannot be read to tell.
// spec: ordin-desktop/components/ProjectService#SA-RECON-12
fn proxy_without_credential(value: &OsStr) -> Option<OsString> {
    let text = value.to_str()?;
    let (scheme, rest) = match text.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, text),
    };
    // The last `@`: a password can hold one, and no proxy address has a path that does.
    let host = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
    Some(match scheme {
        Some(scheme) => format!("{scheme}://{host}").into(),
        None => host.into(),
    })
}

/// The value a listed variable passes on: a proxy address is cleaned, anything else is unchanged.
fn allowed_value(name: &str, value: Option<OsString>) -> Option<OsString> {
    let proxy = matches!(
        name,
        "HTTPS_PROXY" | "HTTP_PROXY" | "https_proxy" | "http_proxy"
    );
    if proxy {
        value.as_deref().and_then(proxy_without_credential)
    } else {
        value
    }
}

/// Replaces a child's environment with `CHECK_ENVIRONMENT`. Used for checks and language servers.
pub(crate) fn set_allowed_environment(command: &mut Command) {
    command.env_clear();
    for name in CHECK_ENVIRONMENT {
        if let Some(value) = allowed_value(name, std::env::var_os(name)) {
            command.env(name, value);
        }
    }
}

/// Runs one command with only `CHECK_ENVIRONMENT` and a limit on time and output (Internal Logic 7.2).
pub(crate) async fn execute(
    folder: &Path,
    command: &str,
    limit: Duration,
) -> Result<ProjectCheckResult, ProjectError> {
    let started = Instant::now();
    let mut shell = Command::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    set_allowed_environment(&mut shell);
    shell_arguments(&mut shell, command);
    shell
        .current_dir(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    shell.process_group(0);
    let mut child = shell.spawn().map_err(|_| ProjectError::CheckDenied {
        reason: "The command could not be started.".to_owned(),
    })?;
    let pid = child.id();
    let captured = Arc::new(Mutex::new(Captured::default()));
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(tokio::spawn(capture(out, captured.clone())));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(tokio::spawn(capture(err, captured.clone())));
    }
    let status = match tokio::time::timeout(limit, child.wait()).await {
        Ok(status) => status.map_err(|_| ProjectError::CheckTimeout)?,
        Err(_) => {
            kill_tree(pid).await;
            let _ = child.kill().await;
            return Err(ProjectError::CheckTimeout);
        }
    };
    for reader in readers {
        let aborter = reader.abort_handle();
        if tokio::time::timeout(OUTPUT_DRAIN, reader).await.is_err() {
            aborter.abort();
        }
    }
    let captured = std::mem::take(&mut *lock(&captured));
    let mut output = String::from_utf8_lossy(&captured.bytes).into_owned();
    if captured.truncated {
        output.push_str("\n[The output was cut: only the first 1 MiB is shown.]\n");
    }
    Ok(ProjectCheckResult {
        command: command.to_owned(),
        exit_code: status.code(),
        output,
        truncated: captured.truncated,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

impl ProjectService {
    /// Where a check runs: the task's worktree, or the project folder for the person.
    async fn check_folder(
        &self,
        project: ProjectId,
        place: ordin_types::PlaceId,
        task: Option<CodingTaskId>,
    ) -> Result<PathBuf, ProjectError> {
        match task {
            Some(task) => {
                let row = self.task_row(task).await?;
                if row.task.project_id != project {
                    return Err(ProjectError::TaskNotFound);
                }
                Ok(PathBuf::from(row.task.worktree_path))
            }
            None => self.place_folder(place).await,
        }
    }

    /// Internal Logic 7.1 for the person, for the agent's `checks.run_check` and for the checks this
    /// service runs for a task.
    // spec: ordin-desktop/components/ProjectService#ADR-021
    // spec: ordin-desktop/components/ProjectService#SA-RECON-03
    pub async fn run_check(
        &self,
        project_id: ProjectId,
        task: Option<CodingTaskId>,
        command: &str,
        origin: CommandOrigin,
    ) -> Result<ProjectCheckResult, ProjectError> {
        let row = self.open_row(project_id).await?;
        let folder = self
            .check_folder(project_id, row.project.place_id, task)
            .await?;
        let limit = lock(&self.tunables).check_timeout;
        let outcome = self
            .rules
            .decide_command(CommandRequest {
                workspace_id: row.workspace_id,
                origin,
                command: command.to_owned(),
                working_dir: folder.clone(),
                declared_check: row.project.check_commands.iter().any(|c| c == command),
            })
            .await;
        match outcome.decision.verdict {
            Verdict::Allow => {}
            Verdict::Deny { reason } => return Err(ProjectError::CheckDenied { reason }),
            Verdict::Ask { .. } => {
                let refused = || ProjectError::CheckDenied {
                    reason: "The person did not approve this command.".to_owned(),
                };
                let approval = outcome.approval_id.ok_or_else(refused)?;
                let until = tokio::time::Instant::now() + limit;
                match self.rules.wait_for_approval(approval, Some(until)).await {
                    Ok(ApprovalWait::Approved) => {}
                    Ok(ApprovalWait::Pending) => return Err(ProjectError::CheckTimeout),
                    Ok(ApprovalWait::Rejected | ApprovalWait::Expired) | Err(_) => {
                        return Err(refused())
                    }
                }
            }
        }
        self.run_decided_check(
            row.project.id,
            row.workspace_id,
            task,
            &folder,
            command,
            limit,
        )
        .await
    }

    /// Runs a command already decided, and records it without its output (Internal Logic 7.3).
    async fn run_decided_check(
        &self,
        project: ProjectId,
        workspace: ordin_types::WorkspaceId,
        task: Option<CodingTaskId>,
        folder: &Path,
        command: &str,
        limit: Duration,
    ) -> Result<ProjectCheckResult, ProjectError> {
        let result = execute(folder, command, limit).await?;
        crate::log::check_run(project, task, result.exit_code, result.elapsed_ms);
        self.record(
            "check_run",
            None,
            Some(workspace),
            "A check command ran.",
            json!({
                "project_id": project.to_string(),
                "task_id": task.map(|t| t.to_string()),
                "exit_code": result.exit_code,
                "elapsed_ms": result.elapsed_ms,
            }),
        )
        .await;
        Ok(result)
    }

    /// `checks.run_check`: the MCP Front decides on the tool and never reads the command, so the
    /// command takes the path of Internal Logic 7.1 with the calling run as its origin.
    pub(crate) async fn run_tool_check(
        &self,
        task: CodingTaskId,
        command: &str,
        origin: CommandOrigin,
    ) -> Result<ProjectCheckResult, ProjectError> {
        let row = self.task_row(task).await?;
        self.run_check(row.task.project_id, Some(task), command, origin)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_proxy_variable_is_cleaned_and_no_other() {
        let secret = OsString::from("http://user:pass@proxy.acme:8080");
        let clean = Some(OsString::from("http://proxy.acme:8080"));
        for name in ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"] {
            assert!(CHECK_ENVIRONMENT.contains(&name));
            assert_eq!(allowed_value(name, Some(secret.clone())), clean, "{name}");
        }
        assert_eq!(
            allowed_value("NO_PROXY", Some(secret.clone())),
            Some(secret)
        );
        assert_eq!(allowed_value("HTTPS_PROXY", None), None);
    }

    #[test]
    fn a_proxy_value_loses_its_credential() {
        for (value, expected) in [
            ("http://user:pass@proxy.acme:8080", "http://proxy.acme:8080"),
            (
                "https://user:p@ss@proxy.acme:8080/",
                "https://proxy.acme:8080/",
            ),
            ("http://user:p/ss@proxy.acme:8080", "http://proxy.acme:8080"),
            ("user:pass@proxy.acme:8080", "proxy.acme:8080"),
            ("http://proxy.acme:8080", "http://proxy.acme:8080"),
            ("proxy.acme:8080", "proxy.acme:8080"),
        ] {
            assert_eq!(
                proxy_without_credential(OsStr::new(value)),
                Some(OsString::from(expected)),
                "{value}"
            );
        }
    }
}
