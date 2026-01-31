//! The program's paths (OrdinDesktop.md *Application data root (desktop)*, *Rust items*) and step 2's
//! data-root check.

use std::path::{Path, PathBuf};

use crate::error::StartError;

/// `<data root>` and the paths fixed under it, and the bundled programs beside the executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPaths {
    pub data_root: PathBuf,
    pub logs: PathBuf,
    pub run: PathBuf,
    /// Socket path (macOS) or pipe name (Windows).
    pub hook_endpoint: String,
    pub hook_program: PathBuf,
    /// Resolved only in a build with `copilot` (DSK-UP-5, `DesktopPaths::copilot_cli` seam).
    #[cfg(feature = "copilot")]
    pub copilot_cli: PathBuf,
    pub preferences: PathBuf,
}

/// The file name of a bundled program on this platform (`ordin-hook.exe` on Windows).
fn program_file(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// The hook channel: `<data root>/run/hook.sock` on macOS, `\\.\pipe\ordin-hook-<installation id>`
/// on Windows (2D-1c `ORDIN_HOOK_ENDPOINT`).
pub fn hook_endpoint(run: &Path, installation_id: &str) -> String {
    if cfg!(windows) {
        format!(r"\\.\pipe\ordin-hook-{installation_id}")
    } else {
        run.join("hook.sock").to_string_lossy().into_owned()
    }
}

impl DesktopPaths {
    /// The paths under `data_root`, with the bundled programs found in `exe_dir` (the folder of
    /// `std::env::current_exe()`).
    pub fn new(data_root: PathBuf, exe_dir: &Path, installation_id: &str) -> DesktopPaths {
        let run = data_root.join("run");
        DesktopPaths {
            logs: data_root.join("logs"),
            hook_endpoint: hook_endpoint(&run, installation_id),
            run,
            hook_program: exe_dir.join(program_file("ordin-hook")),
            #[cfg(feature = "copilot")]
            copilot_cli: exe_dir.join(program_file("copilot")),
            preferences: data_root.join("preferences.json"),
            data_root,
        }
    }

    /// Step 2: create `<data root>`, `logs/` and `run/` (`run/` mode `0700`), then write and delete
    /// `<data root>/.write-probe`. Any failure is `StartError::DataRootUnwritable`.
    pub fn prepare(&self) -> Result<(), StartError> {
        let unwritable = |path: &Path| {
            let path = path.to_path_buf();
            move |source| StartError::DataRootUnwritable { path, source }
        };
        for dir in [&self.data_root, &self.logs, &self.run] {
            std::fs::create_dir_all(dir).map_err(unwritable(dir))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.run, std::fs::Permissions::from_mode(0o700))
                .map_err(unwritable(&self.run))?;
        }
        let probe = self.data_root.join(".write-probe");
        std::fs::write(&probe, b"").map_err(unwritable(&probe))?;
        std::fs::remove_file(&probe).map_err(unwritable(&probe))?;
        Ok(())
    }

    /// The bundled programs of this build that are not beside the executable (step 4: each is
    /// logged as `DSK_SIDECAR_MISSING` and start continues). `copilot` is checked only in a build with
    /// the `copilot` switch.
    pub fn missing_sidecars(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !self.hook_program.is_file() {
            missing.push("ordin-hook");
        }
        #[cfg(feature = "copilot")]
        if !self.copilot_cli.is_file() {
            missing.push("copilot");
        }
        missing
    }

    /// Logs one `DSK_SIDECAR_MISSING` line per missing bundled program, naming its file.
    pub fn log_missing_sidecars(&self) -> Vec<&'static str> {
        let missing = self.missing_sidecars();
        for name in &missing {
            tracing::warn!(
                component = "desktop",
                code = crate::error::SIDECAR_MISSING,
                file = *name,
                "sidecar_missing"
            );
        }
        missing
    }
}

/// The first `claude` program found in `search`, in order, else the bare name (OQ-REC-4 option (b)).
///
/// A program started from Finder, the Dock or the Start menu gets only the system's short `PATH`,
/// which leaves out the folder a person's own Claude Code install lives in. `search` is therefore the
/// `PATH` folders followed by the usual per-user and package-manager folders.
pub fn find_claude(search: &[PathBuf]) -> PathBuf {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    search
        .iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("claude"))
}

/// The folders searched for `claude` beyond `PATH`: the person's own `.local/bin` (home folder from
/// `HOME`, or `USERPROFILE` on Windows) and, on macOS, the two Homebrew folders.
pub fn extra_claude_folders(home: Option<PathBuf>) -> Vec<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    // OQ-REC-4: a `claude` outside `PATH` is still found, and "not found" stays the bare name.
    #[test]
    fn find_claude_takes_the_first_folder_holding_it() {
        let dir = TempDir::new("claude-search");
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        std::fs::create_dir_all(&first).expect("first");
        std::fs::create_dir_all(&second).expect("second");
        let name = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        std::fs::write(second.join(name), b"").expect("claude");
        assert_eq!(
            find_claude(&[first.clone(), second.clone()]),
            second.join(name)
        );
        assert_eq!(find_claude(&[first]), PathBuf::from("claude"));
    }

    #[test]
    fn extra_claude_folders_start_with_the_home_local_bin() {
        let folders = extra_claude_folders(Some(PathBuf::from("/home/a")));
        assert_eq!(folders[0], PathBuf::from("/home/a/.local/bin"));
        assert!(extra_claude_folders(None).iter().all(|f| f.is_absolute()));
    }

    #[test]
    fn the_paths_sit_under_the_data_root() {
        let dir = TempDir::new("paths");
        let paths = DesktopPaths::new(dir.path().join("root"), dir.path(), "id");
        assert_eq!(paths.logs, dir.path().join("root/logs"));
        assert_eq!(paths.run, dir.path().join("root/run"));
        assert_eq!(paths.preferences, dir.path().join("root/preferences.json"));
        assert_eq!(
            paths.hook_program,
            dir.path().join(program_file("ordin-hook"))
        );
        if cfg!(windows) {
            assert_eq!(paths.hook_endpoint, r"\\.\pipe\ordin-hook-id");
        } else {
            assert_eq!(
                paths.hook_endpoint,
                dir.path().join("root/run/hook.sock").to_string_lossy()
            );
        }
    }

    #[test]
    fn prepare_creates_the_folders_and_leaves_no_probe() {
        let dir = TempDir::new("prepare");
        let paths = DesktopPaths::new(dir.path().join("root"), dir.path(), "id");
        paths.prepare().expect("writable");
        assert!(paths.logs.is_dir() && paths.run.is_dir());
        assert!(!paths.data_root.join(".write-probe").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&paths.run)
                .expect("run")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }

    // Error Table row: `<data root>` cannot be created or written.
    #[test]
    fn error_path_data_root_not_writable() {
        let dir = TempDir::new("unwritable");
        // A file where the data root's parent folder should be: nothing can be created under it,
        // whoever runs the test.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"").expect("blocker");
        let paths = DesktopPaths::new(blocker.join("root"), dir.path(), "id");
        let err = paths.prepare().expect_err("unwritable");
        assert_eq!(err.code(), "DSK_DATA_ROOT_UNWRITABLE");
        assert!(matches!(err, StartError::DataRootUnwritable { .. }));
    }

    // Error Table row: a bundled program is missing beside the executable.
    #[test]
    fn error_path_a_missing_bundled_program() {
        let dir = TempDir::new("sidecars");
        let paths = DesktopPaths::new(dir.path().join("root"), dir.path(), "id");
        assert_eq!(paths.log_missing_sidecars().first(), Some(&"ordin-hook"));
        std::fs::write(&paths.hook_program, b"").expect("hook");
        let missing = paths.missing_sidecars();
        assert!(!missing.contains(&"ordin-hook"));
        #[cfg(feature = "copilot")]
        assert_eq!(missing, vec!["copilot"]);
        #[cfg(not(feature = "copilot"))]
        assert!(missing.is_empty());
    }

    // OrdinDesktop.md amendment January 2026 (ADR-114).
    #[cfg(not(feature = "copilot"))]
    #[test]
    fn edge_case_copilot_is_not_offered_without_its_switch() {
        let dir = TempDir::new("no-copilot");
        let paths = DesktopPaths::new(dir.path().join("root"), dir.path(), "id");
        std::fs::write(&paths.hook_program, b"").expect("hook");
        // No copilot file exists, and none is reported missing.
        assert!(paths.log_missing_sidecars().is_empty());
        assert_eq!(crate::plan::bundled_program_names(), ["ordin-hook"]);
    }
}
