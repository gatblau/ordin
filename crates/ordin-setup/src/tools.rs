//! FU-094: the command-line tools release 1 starts from the computer's `PATH` (ADR-072 as
//! amended: `gh` and `glab` are not yet placed under the data root), named with what to install
//! when one is missing. The check reads `PATH` only; it installs, changes and removes nothing.

use std::ffi::OsStr;

use crate::assistant::SetupAssistant;
use crate::process::find_on_path;
use crate::types::MissingTool;

/// (command, what to install).
const PATH_TOOLS: [(&str, &str); 2] = [("gh", "GitHub CLI"), ("glab", "GitLab CLI")];

/// The tools of `PATH_TOOLS` not found as an executable file in any `path` entry; empty when
/// every one is present.
// spec: ordin-desktop/components/SetupAssistant#SET-UP-28
pub fn missing_path_tools(path: Option<&OsStr>) -> Vec<MissingTool> {
    PATH_TOOLS
        .iter()
        .filter(|(command, _)| {
            let names = if cfg!(windows) {
                vec![format!("{command}.exe")]
            } else {
                vec![(*command).to_owned()]
            };
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            find_on_path(path, &names).is_none()
        })
        .map(|(command, install)| MissingTool {
            command: (*command).to_owned(),
            install: (*install).to_owned(),
        })
        .collect()
}

impl SetupAssistant {
    /// FU-094: the tools missing from this process's `PATH`, for the program to show at start.
    pub fn missing_path_tools(&self) -> Vec<MissingTool> {
        missing_path_tools(std::env::var_os("PATH").as_deref())
    }
}
