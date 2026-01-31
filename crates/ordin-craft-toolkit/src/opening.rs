//! Opening a file for the person (CraftToolkit.md *Internal Logic* 4). Programs are started with
//! argument lists, never through a shell.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

use ordin_types::num::Sha256Hex;
use ordin_types::PlaceId;
use serde_json::json;

use crate::definitions::AppWindow;
use crate::error::CraftError;
use crate::model::{BlockedProgram, CraftPlatform, OpenOutcome};
use crate::toolkit::{lock, CraftToolkit};

/// Starts a program detached. A trait so that tests can record what would open.
pub(crate) trait Launcher: Send + Sync {
    fn launch(&self, program: &OsStr, args: &[OsString]) -> std::io::Result<()>;
}

pub(crate) struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn launch(&self, program: &OsStr, args: &[OsString]) -> std::io::Result<()> {
        // No wait and no `kill_on_drop`: the runtime reaps the child after it ends.
        tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
    }
}

/// The command that opens `file` in the app in `folder`; `None` when the definition names no
/// window for this platform.
pub(crate) fn window_command(
    platform: CraftPlatform,
    folder: &Path,
    window: &AppWindow,
    file: &Path,
) -> Option<(OsString, Vec<OsString>)> {
    match platform {
        CraftPlatform::MacosArm64 | CraftPlatform::MacosX86_64 => {
            let bundle = folder.join(window.macos_bundle.as_deref()?);
            Some((
                "open".into(),
                vec!["-a".into(), bundle.into(), file.as_os_str().to_owned()],
            ))
        }
        CraftPlatform::WindowsX86_64 => {
            let exe = folder.join(window.windows_exe.as_deref()?);
            Some((exe.into(), vec![file.as_os_str().to_owned()]))
        }
        CraftPlatform::LinuxX86_64 => {
            let exe = folder.join(window.linux_exe.as_deref()?);
            Some((exe.into(), vec![file.as_os_str().to_owned()]))
        }
    }
}

/// The kinds the system would run rather than show (SA-RECON-11). The same list on every
/// platform, because a file written here is synced and opened elsewhere.
// spec: ordin-desktop/components/CraftToolkit#SA-RECON-11
const RUNNABLE_KINDS: &[&str] = &[
    // macOS
    "afploc",
    "ftploc",
    "mailloc",
    "mobileconfig",
    "action",
    "app",
    "applescript",
    "command",
    "dmg",
    "fileloc",
    "inetloc",
    "mpkg",
    "pkg",
    "prefpane",
    "scpt",
    "scptd",
    "shortcut",
    "terminal",
    "tool",
    "webloc",
    "workflow",
    // Windows
    "accde",
    "ade",
    "adp",
    "appcontent-ms",
    "bas",
    "cer",
    "crt",
    "der",
    "mdb",
    "msh1",
    "msh2",
    "mshxml",
    "msu",
    "ppkg",
    "rdp",
    "vsix",
    "vsto",
    "wsb",
    "appinstaller",
    "application",
    "appref-ms",
    "appx",
    "appxbundle",
    "bat",
    "cab",
    "chm",
    "cmd",
    "com",
    "cpl",
    "diagcab",
    "exe",
    "gadget",
    "hlp",
    "hta",
    "img",
    "inf",
    "ins",
    "iso",
    "isp",
    "js",
    "jse",
    "lnk",
    "mde",
    "msc",
    "msh",
    "msi",
    "msix",
    "msixbundle",
    "msp",
    "mst",
    "ocx",
    "pif",
    "ps1",
    "ps1xml",
    "ps2",
    "ps2xml",
    "psc1",
    "psc2",
    "psd1",
    "psm1",
    "reg",
    "rgs",
    "scf",
    "scr",
    "sct",
    "searchconnector-ms",
    "settingcontent-ms",
    "library-ms",
    "shb",
    "shs",
    "sys",
    "theme",
    "url",
    "vb",
    "vbe",
    "vbs",
    "vhd",
    "vhdx",
    "website",
    "ws",
    "wsc",
    "wsf",
    "wsh",
    "xbap",
    "xll",
    // Office files that run macros or fetch and run content
    "docm",
    "dotm",
    "iqy",
    "pptm",
    "slk",
    "xlam",
    "xlsb",
    "xlsm",
    // Linux
    "appimage",
    "bash",
    "bin",
    "csh",
    "deb",
    "desktop",
    "flatpakref",
    "fish",
    "ksh",
    "rpm",
    "run",
    "sh",
    "snap",
    "zsh",
    // Scripts and archives a language runtime associated with the kind would run
    "pyo",
    "pyzw",
    "jar",
    "jnlp",
    "php",
    "pl",
    "py",
    "pyc",
    "pyw",
    "pyz",
    "rb",
];

/// Whether the system would run `file`. Windows drops a name's trailing dots and spaces and reads
/// what follows a `:` as a stream of the file before it, so both are read as it reads them.
fn runnable_kind(platform: CraftPlatform, file: &Path) -> bool {
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let streamless = match platform {
        CraftPlatform::WindowsX86_64 => name.split(':').next().unwrap_or_default(),
        _ => name.as_str(),
    };
    let name = streamless.trim_end_matches(['.', ' ']);
    name.rsplit_once('.')
        .is_some_and(|(_, kind)| RUNNABLE_KINDS.contains(&kind))
}

/// `explorer.exe` splits its command line at commas into switches and targets, and nothing states
/// how it reads one in a path, so a path holding one is not handed to it.
// spec: ordin-desktop/components/CraftToolkit#SA-RECON-11
fn explorer_misreads(platform: CraftPlatform, file: &Path) -> bool {
    platform == CraftPlatform::WindowsX86_64 && file.to_string_lossy().contains(',')
}

/// macOS and Linux run a file with an execute bit whatever its name, so one is refused too.
#[cfg(unix)]
fn has_execute_bit(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn has_execute_bit(_meta: &std::fs::Metadata) -> bool {
    false
}

pub(crate) fn system_command(platform: CraftPlatform, file: &Path) -> (OsString, Vec<OsString>) {
    let program = match platform {
        CraftPlatform::MacosArm64 | CraftPlatform::MacosX86_64 => "open",
        CraftPlatform::WindowsX86_64 => "explorer.exe",
        CraftPlatform::LinuxX86_64 => "xdg-open",
    };
    (program.into(), vec![file.as_os_str().to_owned()])
}

/// A relative path with no `..`. This is lexical: `resolve` follows links before it trusts the path.
fn relative_ok(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\0')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

impl CraftToolkit {
    /// Internal Logic 4.1: an existing file whose real location lies in the place, spelt from the
    /// place's root. A link is followed first, so one that leaves the place is refused and one
    /// that stays opens its target. `FileService`'s own `confine` is private to that crate.
    async fn resolve(&self, place_id: PlaceId, path: &str) -> Result<PathBuf, CraftError> {
        if !relative_ok(path) {
            return Err(CraftError::OpenFailed);
        }
        let place = self
            .files
            .place(place_id)
            .await
            .map_err(|_| CraftError::OpenFailed)?;
        let root = Path::new(&place.path);
        let failed = |_| CraftError::OpenFailed;
        let real_root = tokio::fs::canonicalize(root).await.map_err(failed)?;
        let real_file = tokio::fs::canonicalize(root.join(path))
            .await
            .map_err(failed)?;
        let inside = real_file
            .strip_prefix(&real_root)
            .map_err(|_| CraftError::OpenFailed)?;
        let meta = tokio::fs::metadata(&real_file).await.map_err(failed)?;
        if !meta.is_file() {
            return Err(CraftError::OpenFailed);
        }
        Ok(root.join(inside))
    }

    /// The app that opens this file, when it is installed and its window is known.
    // An unconfirmed app has no definition, so no known window: the outcome is `NoEditor`.
    fn window_for(&self, file: &Path) -> Option<(String, OsString, Vec<OsString>)> {
        let extension = file.extension()?.to_str()?;
        let app = self.file_kind(extension)?.open_with_app?;
        let row = self.row(&app).filter(|row| row.is_installed())?;
        let version = row.installed_version.as_deref()?;
        let definition = self.definition(&app, version)?;
        let folder = self.version_folder(&app, version)?;
        let (program, args) = window_command(self.platform, &folder, &definition.window, file)?;
        Some((app, program, args))
    }

    // spec: ordin-desktop/components/CraftToolkit#SA-RECON-04
    pub async fn open_in_app(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<OpenOutcome, CraftError> {
        let file = self.resolve(place_id, path).await?;
        let Some((app, program, args)) = self.window_for(&file) else {
            return Ok(OpenOutcome::NoEditor);
        };
        match self.launcher.launch(&program, &args) {
            Ok(()) => {
                self.record(
                    "file_opened_in_app",
                    None,
                    format!("A file was opened in {app}."),
                    json!({ "app": app }),
                )
                .await;
                Ok(OpenOutcome::OpenedInApp { app })
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                let reason = "the operating system refused to start it".to_owned();
                tracing::warn!(
                    component = "craft",
                    program = app.to_string().as_str(),
                    "craft_start_blocked"
                );
                self.note_refused_window(&app, &reason);
                Err(CraftError::StartBlocked {
                    program: app,
                    reason,
                })
            }
            Err(_) => Err(CraftError::OpenFailed),
        }
    }

    /// Keeps the refusal for the Setup Assistant's check (Internal Logic 8.2).
    fn note_refused_window(&self, app: &str, reason: &str) {
        let pin = self.craft_profile();
        let sha256 = pin
            .apps
            .iter()
            .find(|p| p.name == app)
            .and_then(|p| p.platforms.get(&self.platform))
            .map(|asset| asset.sha256.clone());
        if let Some(sha256) = sha256 {
            let program = BlockedProgram {
                program: app.to_owned(),
                publisher: None,
                sha256: Sha256Hex(sha256.0),
                reason: reason.to_owned(),
            };
            lock(&self.state).blocked.insert(app.to_owned(), program);
        }
    }

    pub async fn open_with_system_default(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<OpenOutcome, CraftError> {
        let file = self.resolve(place_id, path).await?;
        let meta = tokio::fs::metadata(&file)
            .await
            .map_err(|_| CraftError::OpenFailed)?;
        if runnable_kind(self.platform, &file)
            || has_execute_bit(&meta)
            || explorer_misreads(self.platform, &file)
        {
            tracing::warn!(component = "craft", "craft_open_refused");
            return Err(CraftError::OpenRefused);
        }
        let (program, args) = system_command(self.platform, &file);
        self.launcher
            .launch(&program, &args)
            .map_err(|_| CraftError::OpenFailed)?;
        Ok(OpenOutcome::OpenedWithSystemDefault)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comma_is_refused_only_where_explorer_reads_the_path() {
        let file = Path::new(r"C:\Users\asha\a,b.txt");
        assert!(explorer_misreads(CraftPlatform::WindowsX86_64, file));
        assert!(!explorer_misreads(CraftPlatform::LinuxX86_64, file));
        assert!(!explorer_misreads(
            CraftPlatform::WindowsX86_64,
            Path::new(r"C:\Users\asha\ab.txt")
        ));
    }

    #[test]
    fn the_review_s_kinds_are_runnable() {
        for kind in [
            "docm",
            "xlsm",
            "xlsb",
            "pptm",
            "dotm",
            "xlam",
            "slk",
            "iqy",
            "rdp",
            "wsb",
            "msu",
            "msh1",
            "msh2",
            "mshxml",
            "ppkg",
            "appcontent-ms",
            "bas",
            "pyzw",
            "pyo",
            "vsix",
            "vsto",
            "mdb",
            "ade",
            "adp",
            "accde",
            "cer",
            "crt",
            "der",
            "mobileconfig",
            "ftploc",
            "afploc",
            "mailloc",
        ] {
            let file = format!("x.{}", kind.to_uppercase());
            assert!(
                runnable_kind(CraftPlatform::LinuxX86_64, Path::new(&file)),
                "{kind}"
            );
        }
    }
}
