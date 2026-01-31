//! Install kind and the other install (OrdinDesktop.md amendment January 2026, DSK-UP-15 and
//! DSK-UP-16), as functions of folders so that both answers are testable without an installer.
//! `TauriUpdater` works the kind out once before `register_updater` and keeps it.

use std::path::{Path, PathBuf};

use ordin_setup::InstallKind;

/// The install folder: the folder holding the running executable on Windows, and the folder holding
/// the `.app` bundle on macOS (`<folder>/<name>.app/Contents/MacOS/<exe>`). Elsewhere, and for an
/// executable outside a bundle, the executable's own folder.
pub fn install_folder(exe: &Path) -> Option<PathBuf> {
    let exe_dir = exe.parent()?;
    if cfg!(target_os = "macos") {
        let contents = exe_dir.parent();
        let bundle = contents.and_then(Path::parent);
        if let (Some(contents), Some(bundle)) = (contents, bundle) {
            let is_bundle = exe_dir.file_name().is_some_and(|n| n == "MacOS")
                && contents.file_name().is_some_and(|n| n == "Contents")
                && bundle.extension().is_some_and(|e| e == "app");
            if is_bundle {
                return bundle.parent().map(Path::to_path_buf);
            }
        }
    }
    Some(exe_dir.to_path_buf())
}

/// Creates and removes one uniquely named empty file in `folder`: success → `PerUser`; failure →
/// `WholeComputer`. The answer depends only on that write.
// spec: ordin-desktop/components/OrdinDesktop#DSK-UP-15
pub fn detect_install_kind(folder: &Path) -> InstallKind {
    let probe = folder.join(format!(".ordin-install-probe-{}", uuid::Uuid::now_v7()));
    let kind = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            match std::fs::remove_file(&probe) {
                Ok(()) => InstallKind::PerUser,
                Err(_) => InstallKind::WholeComputer,
            }
        }
        Err(_) => InstallKind::WholeComputer,
    };
    tracing::info!(
        component = "desktop",
        kind = ordin_types::log::wire_word(&kind).as_str(),
        "install_kind"
    );
    kind
}

/// The folders of each install kind on this platform: macOS `~/Applications` and `/Applications`;
/// Windows `%LOCALAPPDATA%\Programs` and `%ProgramFiles%`. The caller resolves them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallFolders {
    pub per_user: PathBuf,
    pub whole_computer: PathBuf,
}

/// The entry for `product_name` in `folder`: `<product>.app` on macOS, `<product>` elsewhere.
fn product_entry(folder: &Path, product_name: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        folder.join(format!("{product_name}.app"))
    } else {
        folder.join(product_name)
    }
}

/// Looks in the other kind's folder for this build's product name. A copy found at a path other than
/// the running one → `Some(path)`; otherwise `None`. Neither copy is changed.
// spec: ordin-desktop/components/OrdinDesktop#DSK-UP-16
pub fn other_install(
    kind: InstallKind,
    folders: &InstallFolders,
    product_name: &str,
    running: &Path,
) -> Option<PathBuf> {
    let other_folder = match kind {
        InstallKind::PerUser => &folders.whole_computer,
        InstallKind::WholeComputer => &folders.per_user,
    };
    let candidate = product_entry(other_folder, product_name);
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    };
    (candidate.exists() && !same(&candidate, running)).then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn entries(folder: &Path) -> Vec<PathBuf> {
        let mut out: Vec<_> = std::fs::read_dir(folder)
            .expect("folder")
            .map(|e| e.expect("entry").path())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn happy_path_a_folder_the_program_can_write_is_a_per_user_install() {
        let dir = TempDir::new("per-user");
        std::fs::write(dir.path().join("ordin"), b"exe").expect("exe");
        let before = entries(dir.path());
        assert_eq!(detect_install_kind(dir.path()), InstallKind::PerUser);
        assert_eq!(entries(dir.path()), before, "the folder holds no new file");
    }

    #[test]
    fn edge_a_folder_the_program_cannot_write_is_a_whole_computer_install() {
        let dir = TempDir::new("whole-computer");
        // A file where the install folder should be: nothing can be created inside it, whoever runs
        // the test (an administrator's account can write any real folder).
        let not_a_folder = dir.path().join("Program Files");
        std::fs::write(&not_a_folder, b"").expect("file");
        assert_eq!(
            detect_install_kind(&not_a_folder),
            InstallKind::WholeComputer
        );
    }

    #[test]
    fn edge_the_other_install_is_found_and_left_alone() {
        let dir = TempDir::new("other");
        let folders = InstallFolders {
            per_user: dir.path().join("per-user"),
            whole_computer: dir.path().join("whole-computer"),
        };
        std::fs::create_dir_all(&folders.per_user).expect("per-user");
        std::fs::create_dir_all(&folders.whole_computer).expect("whole");
        let running = product_entry(&folders.per_user, "ordin");
        std::fs::write(&running, b"running").expect("running");
        assert_eq!(
            other_install(InstallKind::PerUser, &folders, "ordin", &running),
            None
        );
        let other = product_entry(&folders.whole_computer, "ordin");
        std::fs::write(&other, b"other copy").expect("other");
        assert_eq!(
            other_install(InstallKind::PerUser, &folders, "ordin", &running),
            Some(other.clone())
        );
        assert_eq!(std::fs::read(&other).expect("unchanged"), b"other copy");
        assert_eq!(std::fs::read(&running).expect("unchanged"), b"running");
        // From the whole-computer copy, the per-user one is the other install.
        assert_eq!(
            other_install(InstallKind::WholeComputer, &folders, "ordin", &other),
            Some(running)
        );
    }

    #[test]
    fn the_install_folder_is_the_executable_folder_or_the_bundle_folder() {
        if cfg!(target_os = "macos") {
            assert_eq!(
                install_folder(Path::new(
                    "/Users/a/Applications/ordin.app/Contents/MacOS/ordin"
                )),
                Some(PathBuf::from("/Users/a/Applications"))
            );
        }
        assert_eq!(
            install_folder(Path::new("/opt/ordin/ordin")),
            Some(PathBuf::from("/opt/ordin"))
        );
    }
}
