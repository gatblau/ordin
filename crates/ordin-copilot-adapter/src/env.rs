//! The CLI server's environment (CopilotLocalAdapter.md `start` step 4, CPA-CPX-3), built from an
//! empty map so that no GitHub token reaches the CLI (2D-1c).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

/// Reads one variable of this process; replaced in tests.
pub(crate) type ParentEnv = Arc<dyn Fn(&str) -> Option<OsString> + Send + Sync>;

/// Copied when set.
const ALLOWED: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
];

/// Copied on Windows only.
const ALLOWED_WINDOWS: &[&str] = &[
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "SystemRoot",
    "TEMP",
];

/// Never present, whatever the allow-list says.
const NEVER: &[&str] = &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"];

#[cfg(windows)]
const PATH_SEPARATOR: char = ';';
#[cfg(not(windows))]
const PATH_SEPARATOR: char = ':';

/// Step 4's environment, with the folder of `cli_path` at the end of `PATH` when no entry is already
/// that folder (CPA-CPX-3; SetupAssistant.md SET-R1-8).
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CPX-3
pub(crate) fn server_environment(parent: &ParentEnv, cli_path: &Path) -> BTreeMap<String, String> {
    let names = ALLOWED
        .iter()
        .chain(if cfg!(windows) { ALLOWED_WINDOWS } else { &[] });
    let mut env = BTreeMap::new();
    for name in names {
        if NEVER.contains(name) {
            continue;
        }
        if let Some(value) = parent(name) {
            env.insert((*name).to_owned(), value.to_string_lossy().into_owned());
        }
    }
    if let Some(folder) = cli_path.parent().filter(|f| !f.as_os_str().is_empty()) {
        let folder = folder.to_string_lossy().into_owned();
        let path = env.entry("PATH".to_owned()).or_default();
        if !path.split(PATH_SEPARATOR).any(|entry| entry == folder) {
            if !path.is_empty() {
                path.push(PATH_SEPARATOR);
            }
            path.push_str(&folder);
        }
    }
    env
}
