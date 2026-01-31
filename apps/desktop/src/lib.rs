//! The ordin desktop program (OrdinDesktop.md): the composition root that constructs every module of
//! the desktop in the order of *Internal Logic* step 3. This library holds the parts that need no
//! window, so that they are tested without Tauri; `main.rs` adds the Tauri shell on macOS and Windows.

pub mod bundled;
mod confluence_preview;
pub mod core;
pub mod error;
pub mod groups;
pub mod hosts;
pub mod hosts_home;
pub mod hosts_knowledge;
pub mod hosts_run;
pub mod http;
pub mod install_kind;
pub mod memory_models;
pub mod migrations;
pub mod paths;
pub mod plan;
pub mod prefs;
pub mod recovery;
pub mod service_api;
pub mod shell;
pub mod wiring;
pub mod words;
// The window shell: built for macOS and Windows only (HLD 05), where Tauri is a dependency.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod updater;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod window;
// The real construction covers the rows no build switch names; a build with a switch has rows the
// runner does not construct yet.
#[cfg(all(
    test,
    not(any(
        feature = "sandbox",
        feature = "dictation",
        feature = "copilot",
        feature = "slack",
        feature = "mail",
        feature = "team-knowledge",
        feature = "authoring",
        feature = "handover",
        feature = "telegram",
        feature = "hosted-agents",
        feature = "knowledge-agents"
    ))
))]
mod hosts_tests;
#[cfg(all(
    test,
    not(any(
        feature = "sandbox",
        feature = "dictation",
        feature = "copilot",
        feature = "slack",
        feature = "mail",
        feature = "team-knowledge",
        feature = "authoring",
        feature = "handover",
        feature = "telegram",
        feature = "hosted-agents",
        feature = "knowledge-agents"
    ))
))]
mod lifecycle_tests;
#[cfg(all(
    test,
    not(any(
        feature = "sandbox",
        feature = "dictation",
        feature = "copilot",
        feature = "slack",
        feature = "mail",
        feature = "team-knowledge",
        feature = "authoring",
        feature = "handover",
        feature = "telegram",
        feature = "hosted-agents",
        feature = "knowledge-agents"
    ))
))]
mod service_api_rows_tests;
#[cfg(all(
    test,
    not(any(
        feature = "sandbox",
        feature = "dictation",
        feature = "copilot",
        feature = "slack",
        feature = "mail",
        feature = "team-knowledge",
        feature = "authoring",
        feature = "handover",
        feature = "telegram",
        feature = "hosted-agents",
        feature = "knowledge-agents"
    ))
))]
mod wiring_tests;

pub use crate::core::{construct, AfterQuit, Core, QuitReason, ShutdownSummary, StepRunner, Wired};
pub use error::{StartError, ALL_CODES};
pub use groups::groups;
pub use paths::DesktopPaths;
pub use service_api::{DesktopErrorSentences, DesktopServiceApi};
pub use wiring::{CoreInputs, ModuleRunner};

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    /// A folder for one test, removed on drop. On macOS it sits in `~/.ordin-test`: the system's
    /// temporary folder resolves under `/private`, a protected location (a workspace's own folder is
    /// refused there), and its path leaves too little of the 104 bytes of a Unix socket path for
    /// `<folder>/run/mcp.sock`. Elsewhere it sits under the system's temporary folder.
    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(label: &str) -> TempDir {
            let base = match std::env::var_os("HOME") {
                Some(home) if cfg!(target_os = "macos") => PathBuf::from(home).join(".ordin-test"),
                _ => std::env::temp_dir(),
            };
            // The last 12 characters of a version 7 identifier are random; the first are the time.
            let id = uuid::Uuid::now_v7().simple().to_string();
            let dir = base.join(format!("ordin-{label}-{}", &id[20..]));
            std::fs::create_dir_all(&dir).expect("temporary folder");
            TempDir(dir)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
