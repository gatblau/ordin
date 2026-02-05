//! The program's implementations of the Copilot traits (OrdinDesktop.md *Program-side
//! implementations and calls*, rows 3.33 and 3.43, amended by DSK-CPX-3 and DSK-CPX-4; ADR-123): the
//! Copilot Local Adapter's `CopilotCliSource` and the Setup Assistant's Copilot `HarnessProbe`. Both
//! find the Copilot CLI the person installed with the same `locate`, so the probe and the adapter
//! never disagree; nothing is cached between calls, so a CLI installed while ordin is open is found by
//! the next check or session (SET-CPX-5).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ordin_copilot_adapter::{
    CopilotAdapterError, CopilotCli, CopilotCliSource, CopilotLocalAdapter,
};
use ordin_setup::{
    HarnessKind, HarnessLocation, HarnessProbe, HarnessSignIn, ProbeError, ProbeRepair,
};
use tokio::process::Command;

/// `<found> --version`: 10 seconds (SET-CPX-3).
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// Where `copilot` is looked for: this process's `PATH` and the folders of SET-CPX-2. Tests give
/// their own; the program reads its environment at every call.
#[derive(Debug, Clone, Default)]
pub struct CopilotLookup {
    /// `None` reads this process's own at each call.
    pub path: Option<OsString>,
    pub home: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub app_data: Option<PathBuf>,
    /// The environment is read from this process at each call.
    pub from_process: bool,
}

impl CopilotLookup {
    /// This process's `PATH`, home folder (`USERPROFILE` on Windows), `LOCALAPPDATA` and `APPDATA`.
    pub fn from_process() -> CopilotLookup {
        CopilotLookup {
            from_process: true,
            ..CopilotLookup::default()
        }
    }

    fn resolved(&self) -> CopilotLookup {
        if !self.from_process {
            return self.clone();
        }
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        CopilotLookup {
            path: var("PATH"),
            home: var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from),
            local_app_data: var("LOCALAPPDATA").map(PathBuf::from),
            app_data: var("APPDATA").map(PathBuf::from),
            from_process: false,
        }
    }

    /// DSK-CPX-3: `find_copilot_cli`, then `<found> --version` with the D.1 environment, the found
    /// folder added at the end of `PATH` (SET-R1-8), null stdin and a 10 s timeout. Nothing found →
    /// `Ok(None)`; output with no version token → `ProbeError::Unreadable`.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-CPX-3
    // spec: ordin-desktop/components/SetupAssistant#SET-CPX-3
    pub async fn locate(&self) -> Result<Option<HarnessLocation>, ProbeError> {
        let lookup = self.resolved();
        let Some(found) = ordin_setup::find_copilot_cli(
            lookup.path.as_deref(),
            lookup.home.as_deref(),
            lookup.local_app_data.as_deref(),
            lookup.app_data.as_deref(),
        ) else {
            return Ok(None);
        };
        let mut command = Command::new(&found);
        command
            .arg("--version")
            .env_clear()
            .envs(version_env(&lookup, found.parent()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let child = match command.spawn() {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(ProbeError::Refused)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ProbeError::Unreadable),
        };
        let output = match tokio::time::timeout(VERSION_TIMEOUT, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(_)) => return Err(ProbeError::Unreadable),
            Err(_) => return Err(ProbeError::Timeout),
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = version_token(&stdout).ok_or(ProbeError::Unreadable)?;
        Ok(Some(HarnessLocation {
            path: found,
            version,
        }))
    }
}

/// The D.1 environment (SetupAssistant.md 2D-1c): `PATH` with `folder` added at the end when no entry
/// is already that folder (SET-R1-8), the home folder, the account name and `LANG`; on Windows also
/// the four folders Node.js needs to start (SET-R1-4). No credential variable is passed.
fn version_env(lookup: &CopilotLookup, folder: Option<&Path>) -> Vec<(&'static str, OsString)> {
    let mut env = Vec::new();
    let mut entries: Vec<PathBuf> = lookup
        .path
        .as_deref()
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    if let Some(folder) = folder.filter(|f| !f.as_os_str().is_empty()) {
        if !entries.iter().any(|e| e == folder) {
            entries.push(folder.to_path_buf());
        }
    }
    if let Ok(path) = std::env::join_paths(entries) {
        env.push(("PATH", path));
    }
    if let Some(home) = &lookup.home {
        env.push((
            if cfg!(windows) { "USERPROFILE" } else { "HOME" },
            home.clone().into_os_string(),
        ));
    }
    let user = if cfg!(windows) { "USERNAME" } else { "USER" };
    if let Some(u) = std::env::var_os(user) {
        env.push((user, u));
    }
    if let Some(l) = std::env::var_os("LANG") {
        env.push(("LANG", l));
    }
    #[cfg(windows)]
    for name in ["APPDATA", "LOCALAPPDATA", "SystemRoot", "TEMP"] {
        if let Some(value) = std::env::var_os(name) {
            env.push((name, value));
        }
    }
    env
}

/// The first whitespace-separated token matching `^[0-9]+\.[0-9]+\.[0-9]+` (SET-CPX-3, SA-CPX-01).
fn version_token(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let mut parts = token.splitn(3, '.');
        let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        let major = parts.next()?;
        let minor = parts.next()?;
        let rest = parts.next()?;
        let patch_starts = rest.chars().next().is_some_and(|c| c.is_ascii_digit());
        (digits(major) && digits(minor) && patch_starts).then(|| token.to_owned())
    })
}

/// `CopilotCliSource` for the desktop: the probe's own `locate` (DSK-CPX-4).
// spec: ordin-desktop/components/OrdinDesktop#DSK-CPX-4
pub struct DesktopCopilotCli {
    pub lookup: CopilotLookup,
}

#[async_trait]
impl CopilotCliSource for DesktopCopilotCli {
    async fn current(&self) -> Option<CopilotCli> {
        match self.lookup.locate().await {
            Ok(Some(found)) => Some(CopilotCli {
                path: found.path,
                version: found.version,
            }),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(
                    component = "desktop",
                    reason = format!("{error:?}").as_str(),
                    "copilot_cli_unreadable"
                );
                None
            }
        }
    }
}

/// The Copilot `HarnessProbe` registered right after row 3.43 (DSK-CPX-3): `locate` as above; the
/// sign-in calls over the Copilot Local Adapter's probe calls.
pub struct DesktopCopilotProbe {
    pub lookup: CopilotLookup,
    pub adapter: Arc<CopilotLocalAdapter>,
}

/// `ProfileNotOffered` or `ServerSignInUnconfirmed` → `NotOffered`, any other → `Unreadable`
/// (OrdinDesktop.md, *chosen here*).
fn probe_error(error: &CopilotAdapterError) -> ProbeError {
    match error {
        CopilotAdapterError::ProfileNotOffered | CopilotAdapterError::ServerSignInUnconfirmed => {
            ProbeError::NotOffered
        }
        _ => ProbeError::Unreadable,
    }
}

#[async_trait]
impl HarnessProbe for DesktopCopilotProbe {
    fn harness(&self) -> HarnessKind {
        HarnessKind::CopilotCli
    }

    async fn locate(&self) -> Result<Option<HarnessLocation>, ProbeError> {
        self.lookup.locate().await
    }

    async fn sign_in_status(&self) -> Result<HarnessSignIn, ProbeError> {
        let status = self
            .adapter
            .probe_sign_in()
            .await
            .map_err(|e| probe_error(&e))?;
        Ok(if status.signed_in {
            HarnessSignIn::SignedIn {
                method: status.method,
            }
        } else {
            HarnessSignIn::SignedOut
        })
    }

    async fn start_sign_in(&self) -> Result<(), ProbeError> {
        self.adapter
            .probe_start_sign_in()
            .await
            .map_err(|e| probe_error(&e))
    }

    async fn repair(&self) -> Result<ProbeRepair, ProbeError> {
        Ok(match self.adapter.probe_repair().await {
            Ok(()) => ProbeRepair::Repaired,
            Err(_) => ProbeRepair::Failed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_first_semantic_version_token() {
        assert_eq!(version_token("1.0.98\n").as_deref(), Some("1.0.98"));
        assert_eq!(
            version_token("GitHub Copilot CLI 1.0.98.\n").as_deref(),
            Some("1.0.98.")
        );
        assert_eq!(version_token("version unknown"), None);
        assert_eq!(version_token("1.0"), None);
    }

    #[test]
    fn the_found_folder_is_added_to_path_once() {
        let lookup = CopilotLookup {
            path: Some(OsString::from("/usr/bin")),
            ..CopilotLookup::default()
        };
        let path_of = |folder: &str| {
            version_env(&lookup, Some(Path::new(folder)))
                .into_iter()
                .find(|(k, _)| *k == "PATH")
                .map(|(_, v)| std::env::split_paths(&v).collect::<Vec<_>>())
                .expect("PATH")
        };
        assert_eq!(
            path_of("/home/p/.local/bin"),
            [
                PathBuf::from("/usr/bin"),
                PathBuf::from("/home/p/.local/bin")
            ]
        );
        assert_eq!(path_of("/usr/bin"), [PathBuf::from("/usr/bin")]);
    }
}
