//! IDEs (ADR-052, C-32; Internal Logic 12). Argument lists only, never a shell; ordin writes no IDE
//! setting.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

use ordin_types::ProjectId;
use tokio::process::Command;

use crate::error::ProjectError;
use crate::service::{lock, ProjectService};
use crate::store;
use crate::types::Ide;

/// The macOS bundle names of Internal Logic 12.1: a name prefix, or the whole name.
const MAC_PREFIXES: [&str; 8] = [
    "IntelliJ IDEA",
    "GoLand",
    "RustRover",
    "WebStorm",
    "PyCharm",
    "CLion",
    "Rider",
    "PhpStorm",
];
const MAC_VS_CODE: &str = "Visual Studio Code.app";

/// How an IDE starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Launch {
    /// macOS `open -na <bundle> --args <path>`.
    Bundle(PathBuf),
    /// A launcher taking the path as its argument.
    Launcher(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedIde {
    pub id: String,
    pub name: String,
    pub launch: Launch,
}

#[derive(Default)]
pub(crate) struct IdeCache {
    detected: Option<(Instant, Vec<DetectedIde>)>,
    /// C-32: IDEs whose start failed; *Open in [IDE]* is then not shown for them.
    dropped: HashSet<String>,
}

fn id_of(name: &str) -> String {
    name.to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn entries(folder: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = read
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            Some((name, entry.path()))
        })
        .collect();
    found.sort();
    found
}

fn mac_bundles(folder: &Path) -> Vec<DetectedIde> {
    entries(folder)
        .into_iter()
        .filter(|(name, _)| {
            name == MAC_VS_CODE
                || (name.ends_with(".app") && MAC_PREFIXES.iter().any(|p| name.starts_with(p)))
        })
        .map(|(file, path)| {
            let name = file.trim_end_matches(".app").to_owned();
            DetectedIde {
                id: id_of(&name),
                name,
                launch: Launch::Bundle(path),
            }
        })
        .collect()
}

/// `/Applications` and `~/Applications`, including `~/Applications/JetBrains Toolbox`.
pub(crate) fn detect_mac(system: &Path, home: Option<&Path>) -> Vec<DetectedIde> {
    let mut found = mac_bundles(system);
    if let Some(home) = home {
        let personal = home.join("Applications");
        found.extend(mac_bundles(&personal));
        found.extend(mac_bundles(&personal.join("JetBrains Toolbox")));
    }
    found
}

fn launcher(name: &str, program: PathBuf) -> DetectedIde {
    DetectedIde {
        id: id_of(name),
        name: name.to_owned(),
        launch: Launch::Launcher(program),
    }
}

fn program_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `code` on `PATH` and `~/.local/share/JetBrains/Toolbox/scripts/*`.
pub(crate) fn detect_linux(
    path: Option<&std::ffi::OsStr>,
    home: Option<&Path>,
) -> Vec<DetectedIde> {
    let mut found = Vec::new();
    if let Some(path) = path {
        if let Some(code) = std::env::split_paths(path)
            .map(|folder| folder.join("code"))
            .find(|candidate| candidate.is_file())
        {
            found.push(launcher("Visual Studio Code", code));
        }
    }
    if let Some(home) = home {
        let scripts = home.join(".local/share/JetBrains/Toolbox/scripts");
        for (name, program) in entries(&scripts) {
            found.push(launcher(&name, program));
        }
    }
    found
}

/// Internal Logic 12.1, Windows.
pub(crate) fn detect_windows(
    local_app_data: Option<&Path>,
    program_files: Option<&Path>,
) -> Vec<DetectedIde> {
    let mut found = Vec::new();
    let code = "Microsoft VS Code/Code.exe";
    if let Some(local) = local_app_data {
        let installed = local.join("Programs").join(code);
        if installed.is_file() {
            found.push(launcher("Visual Studio Code", installed));
        }
        for (name, program) in entries(&local.join("JetBrains/Toolbox/scripts")) {
            if name.ends_with(".cmd") {
                found.push(launcher(&program_stem(&program), program));
            }
        }
    }
    if let Some(files) = program_files {
        let installed = files.join(code);
        if installed.is_file() {
            found.push(launcher("Visual Studio Code", installed));
        }
        for (product, folder) in entries(&files.join("JetBrains")) {
            for (name, program) in entries(&folder.join("bin")) {
                if name.ends_with("64.exe") {
                    found.push(launcher(&product, program));
                }
            }
        }
    }
    found
}

fn detect(system: &Path, home: Option<&Path>) -> Vec<DetectedIde> {
    if cfg!(target_os = "macos") {
        detect_mac(system, home)
    } else if cfg!(windows) {
        detect_windows(
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .as_deref(),
            std::env::var_os("ProgramFiles")
                .map(PathBuf::from)
                .as_deref(),
        )
    } else {
        detect_linux(std::env::var_os("PATH").as_deref(), home)
    }
}

impl ProjectService {
    /// Cached 10 minutes; an IDE whose start failed stays off the list (C-32).
    async fn detected_ides(&self) -> Vec<DetectedIde> {
        let cached = {
            let cache = lock(&self.ide_cache);
            cache
                .detected
                .as_ref()
                .filter(|(at, _)| at.elapsed() < crate::IDE_CACHE)
                .map(|(_, list)| list.clone())
        };
        let list = match cached {
            Some(list) => list,
            None => {
                let (system, home) = {
                    let tunables = lock(&self.tunables);
                    (tunables.system_applications.clone(), tunables.home.clone())
                };
                let list = tokio::task::spawn_blocking(move || detect(&system, home.as_deref()))
                    .await
                    .unwrap_or_default();
                lock(&self.ide_cache).detected = Some((Instant::now(), list.clone()));
                list
            }
        };
        let cache = lock(&self.ide_cache);
        list.into_iter()
            .filter(|ide| !cache.dropped.contains(&ide.id))
            .collect()
    }

    pub async fn ides(&self) -> Result<Vec<Ide>, ProjectError> {
        let mut out = Vec::new();
        for ide in self.detected_ides().await {
            let enabled = store::ide_choice(&self.pool, &ide.id)
                .await?
                .unwrap_or(true);
            out.push(Ide {
                id: ide.id,
                name: ide.name,
                enabled,
            });
        }
        Ok(out)
    }

    /// Stores the Settings › Code › *Open code in* choice.
    pub async fn set_ide_enabled(&self, ide_id: &str, enabled: bool) -> Result<(), ProjectError> {
        if !self
            .detected_ides()
            .await
            .iter()
            .any(|ide| ide.id == ide_id)
        {
            return Err(ProjectError::IdeNotFound);
        }
        store::set_ide_choice(&self.pool, ide_id, enabled).await
    }

    /// Internal Logic 12.2.
    // spec: ordin-desktop/components/ProjectService#C-32
    pub async fn open_in_ide(
        &self,
        project_id: ProjectId,
        ide_id: &str,
        path: Option<&str>,
    ) -> Result<(), ProjectError> {
        let row = self.open_row(project_id).await?;
        let ide = self
            .detected_ides()
            .await
            .into_iter()
            .find(|ide| ide.id == ide_id)
            .ok_or(ProjectError::IdeNotFound)?;
        let folder = self.place_folder(row.project.place_id).await?;
        let target = match path {
            Some(path) if crate::git::relative_path_ok(path) => folder.join(path),
            Some(_) => return Err(ProjectError::IdeStartFailed),
            None => folder,
        };
        let mut command = match &ide.launch {
            Launch::Bundle(bundle) => {
                let mut open = Command::new(lock(&self.tunables).open_program.clone());
                open.arg("-na").arg(bundle).arg("--args").arg(&target);
                open
            }
            Launch::Launcher(program) => {
                let mut start = Command::new(program);
                start.arg(&target);
                start
            }
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for variable in crate::CREDENTIAL_VARIABLES {
            command.env_remove(variable);
        }
        let started = match &ide.launch {
            // `open` returns once the app has started, and reports a failure in its exit status.
            Launch::Bundle(_) => command.status().await.is_ok_and(|s| s.success()),
            // A launcher may stay for as long as the IDE runs.
            Launch::Launcher(_) => command.spawn().is_ok(),
        };
        if started {
            crate::log::ide_opened(ide_id);
            Ok(())
        } else {
            lock(&self.ide_cache).dropped.insert(ide.id);
            crate::log::ide_dropped(ide_id);
            Err(ProjectError::IdeStartFailed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("ordin-ide-{name}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn stand_in(fixture: &crate::tests::support::Fixture, launch: Launch) -> String {
        let ide = DetectedIde {
            id: "stand-in".to_owned(),
            name: "Stand In".to_owned(),
            launch,
        };
        let id = ide.id.clone();
        lock(&fixture.service.ide_cache).detected = Some((Instant::now(), vec![ide]));
        id
    }

    #[tokio::test]
    async fn an_ide_opens_the_project_and_the_choice_is_kept() {
        let f = crate::tests::support::fixture().await;
        let (project, _) = f.project().await;
        // A program that takes the folder and ends: `cmd.exe` ends at once with no input.
        let launcher = if cfg!(windows) {
            "cmd.exe"
        } else {
            "/bin/echo"
        };
        let id = stand_in(&f, Launch::Launcher(launcher.into()));
        f.service.open_in_ide(project.id, &id, None).await.unwrap();
        f.service
            .open_in_ide(project.id, &id, Some("src/rates.rs"))
            .await
            .unwrap();
        assert!(f.service.ides().await.unwrap()[0].enabled);
        f.service.set_ide_enabled(&id, false).await.unwrap();
        assert!(!f.service.ides().await.unwrap()[0].enabled);
        let climbing = f.service.open_in_ide(project.id, &id, Some("../x")).await;
        assert_eq!(climbing.unwrap_err().code(), "PROJ_IDE_START_FAILED");
    }

    #[tokio::test]
    async fn an_ide_whose_start_fails_is_dropped_from_the_list() {
        let f = crate::tests::support::fixture().await;
        let (project, _) = f.project().await;
        let id = stand_in(&f, Launch::Launcher("/no/such/launcher".into()));
        let error = f
            .service
            .open_in_ide(project.id, &id, None)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "PROJ_IDE_START_FAILED");
        assert!(error.retryable());
        assert!(f.service.ides().await.unwrap().is_empty());
        let again = f
            .service
            .open_in_ide(project.id, &id, None)
            .await
            .unwrap_err();
        assert_eq!(again.code(), "PROJ_IDE_NOT_FOUND");
    }

    // Unix only: the stand-ins for `open` are `true` and `false`, which Windows does not have.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_bundle_starts_through_open_and_its_exit_status_counts() {
        let f = crate::tests::support::fixture().await;
        let (project, _) = f.project().await;
        lock(&f.service.tunables).open_program = "/usr/bin/true".into();
        let id = stand_in(&f, Launch::Bundle("/Applications/Stand In.app".into()));
        f.service.open_in_ide(project.id, &id, None).await.unwrap();
        lock(&f.service.tunables).open_program = "/usr/bin/false".into();
        let error = f
            .service
            .open_in_ide(project.id, &id, None)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "PROJ_IDE_START_FAILED");
    }

    #[tokio::test]
    async fn detection_runs_once_and_is_cached() {
        let f = crate::tests::support::fixture().await;
        let (system, home) = (folder("cache-system"), folder("cache-home"));
        {
            let mut tunables = lock(&f.service.tunables);
            tunables.system_applications = system;
            tunables.home = Some(home);
        }
        assert!(f.service.ides().await.unwrap().is_empty());
        assert!(lock(&f.service.ide_cache).detected.is_some());
    }

    #[test]
    fn the_macos_table_finds_named_bundles_in_each_folder() {
        let (system, home) = (folder("system"), folder("home"));
        std::fs::create_dir_all(system.join("GoLand 2026.1.app")).unwrap();
        std::fs::create_dir_all(system.join("Visual Studio Code.app")).unwrap();
        std::fs::create_dir_all(system.join("Calculator.app")).unwrap();
        std::fs::create_dir_all(home.join("Applications/RustRover.app")).unwrap();
        std::fs::create_dir_all(home.join("Applications/JetBrains Toolbox/CLion.app")).unwrap();
        let found = detect_mac(&system, Some(&home));
        let names: Vec<_> = found.iter().map(|ide| ide.name.as_str()).collect();
        assert_eq!(
            names,
            ["GoLand 2026.1", "Visual Studio Code", "RustRover", "CLion"]
        );
        assert_eq!(found[1].id, "visual-studio-code");
        assert!(matches!(found[0].launch, Launch::Bundle(_)));
    }

    #[test]
    fn the_linux_table_finds_code_and_toolbox_scripts() {
        let (bin, home) = (folder("bin"), folder("linux-home"));
        std::fs::write(bin.join("code"), "").unwrap();
        let scripts = home.join(".local/share/JetBrains/Toolbox/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("goland"), "").unwrap();
        let found = detect_linux(Some(bin.as_os_str()), Some(&home));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].launch, Launch::Launcher(bin.join("code")));
        assert_eq!(found[1].name, "goland");
    }

    #[test]
    fn the_windows_table_finds_each_location() {
        let (local, files) = (folder("local"), folder("files"));
        std::fs::create_dir_all(local.join("Programs/Microsoft VS Code")).unwrap();
        std::fs::write(local.join("Programs/Microsoft VS Code/Code.exe"), "").unwrap();
        std::fs::create_dir_all(local.join("JetBrains/Toolbox/scripts")).unwrap();
        std::fs::write(local.join("JetBrains/Toolbox/scripts/idea.cmd"), "").unwrap();
        std::fs::create_dir_all(files.join("Microsoft VS Code")).unwrap();
        std::fs::write(files.join("Microsoft VS Code/Code.exe"), "").unwrap();
        std::fs::create_dir_all(files.join("JetBrains/Rider/bin")).unwrap();
        std::fs::write(files.join("JetBrains/Rider/bin/rider64.exe"), "").unwrap();
        let found = detect_windows(Some(&local), Some(&files));
        let names: Vec<_> = found.iter().map(|ide| ide.name.as_str()).collect();
        assert_eq!(
            names,
            ["Visual Studio Code", "idea", "Visual Studio Code", "Rider"]
        );
    }
}
