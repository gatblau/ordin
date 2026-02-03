//! The program's implementations of the two traits the Sandbox owns (OrdinDesktop.md *Program-side
//! implementations and calls*, row 3.27; Sandbox.md SA-RECON-05, SA-CLOSE-04): `SandboxGrants` and
//! `PlaceFiles`. Each delegates to the module the row names and adds no decision (SA-RECON-09).
//
// SPEC-GAP-37: `SandboxGrants::sandbox_capabilities` is "from the Package Inspector's declared
// connections, the organisation profile and Identity" (OrdinDesktop.md row 3.27, RulesEngine.md
// SA-RECON-02), but a declared connection (`ConnectionEntry`: name, server type, transport,
// needs-credential) carries no host, path list, methods, server address, tool list, write or
// private-network permission, or credential kind, and no spec states where those come from. The
// adapter answers no system, which the trait defines as "Empty = no system": sandboxed code runs
// Python with `ordin_tools` and files, and calls no company system, until the spec states the mapping.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use ordin_agent_folders::AgentFolderManager;
use ordin_files::{FileError, FileService};
use ordin_sandbox::{
    GrantsUnavailable, PlaceFileError, PlaceFiles, SandboxCapability, SandboxGrants,
};
use ordin_types::num::Sha256Hex;
use ordin_types::{AgentInstanceId, PlaceId, WorkspaceId};
use sha2::{Digest, Sha256};

/// `SandboxGrants` for the desktop (SPEC-GAP-37).
// spec: ordin-desktop/components/Sandbox#SA-RECON-05
pub struct DesktopSandboxGrants;

#[async_trait]
impl SandboxGrants for DesktopSandboxGrants {
    async fn sandbox_capabilities(
        &self,
        _agent_instance_id: AgentInstanceId,
        _workspace_id: WorkspaceId,
    ) -> Result<Vec<SandboxCapability>, GrantsUnavailable> {
        // SPEC-GAP-37: no system until the spec maps a declared connection to a capability.
        Ok(Vec::new())
    }
}

/// `PlaceFiles` over the File Service's grant-checked read and the Agent Folder Manager's folder.
// spec: ordin-desktop/components/OrdinDesktop#SA-CLOSE-04
pub struct DesktopPlaceFiles {
    pub files: Arc<FileService>,
    pub agent_folders: Arc<AgentFolderManager>,
}

/// The File Service's refusal as the Sandbox's: a path outside the place is `NotFound` (trait text).
fn place_error(error: FileError) -> PlaceFileError {
    match error {
        FileError::NotGranted => PlaceFileError::NotGranted,
        FileError::NotFound | FileError::PlaceNotFound | FileError::PathEscapesPlace => {
            PlaceFileError::NotFound
        }
        FileError::TooLarge { .. } => PlaceFileError::TooLarge,
        _ => PlaceFileError::Unreadable,
    }
}

/// `relative_path` under `folder`, when it is '/'-separated plain names only (no root, no `..`).
fn inside(folder: &Path, relative_path: &str) -> Option<PathBuf> {
    if relative_path.is_empty() || relative_path.contains('\\') {
        return None;
    }
    let relative = Path::new(relative_path);
    let plain = relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    plain.then(|| folder.join(relative))
}

/// Creates the folders between `root` and `target` one at a time and returns `target`'s folder.
/// A step that exists as anything but a real folder (a symbolic link, a file) is refused, so a
/// write never leaves the agent's folder through a link placed inside it.
fn make_folders(root: &Path, target: &Path) -> std::io::Result<PathBuf> {
    let refused = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let relative = target.strip_prefix(root).map_err(|_| refused())?;
    let mut folder = root.to_path_buf();
    let mut steps: Vec<_> = relative.components().collect();
    steps.pop();
    for step in steps {
        folder.push(step);
        match std::fs::symlink_metadata(&folder) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => return Err(refused()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&folder)?,
            Err(e) => return Err(e),
        }
    }
    // The file's own name may not be a link either: rename replaces a link, but refuse it plainly.
    match std::fs::symlink_metadata(target) {
        Ok(meta) if !meta.file_type().is_file() => Err(refused()),
        _ => Ok(folder),
    }
}

#[async_trait]
impl PlaceFiles for DesktopPlaceFiles {
    async fn read_for_agent(
        &self,
        agent_instance_id: AgentInstanceId,
        workspace_id: WorkspaceId,
        place_id: PlaceId,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PlaceFileError> {
        self.files
            .read_for_agent(
                agent_instance_id,
                workspace_id,
                place_id,
                relative_path,
                max_bytes,
            )
            .await
            .map_err(place_error)
    }

    async fn write_agent_output(
        &self,
        agent_instance_id: AgentInstanceId,
        relative_path: &str,
        bytes: &[u8],
    ) -> Result<Sha256Hex, PlaceFileError> {
        let folder = self
            .agent_folders
            .get(agent_instance_id)
            .await
            .map_err(|_| PlaceFileError::Unreadable)?;
        let target =
            inside(&folder.folder_path, relative_path).ok_or(PlaceFileError::Unreadable)?;
        let root = folder.folder_path.clone();
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        let data = bytes.to_vec();
        // Written beside the target and renamed over it, so an earlier file is replaced whole.
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let parent = make_folders(&root, &target)?;
            let temp = parent.join(format!(
                ".ordin-sandbox-{}.tmp",
                uuid::Uuid::now_v7().simple()
            ));
            if let Err(e) =
                std::fs::write(&temp, &data).and_then(|()| std::fs::rename(&temp, &target))
            {
                let _ = std::fs::remove_file(&temp);
                return Err(e);
            }
            Ok(())
        })
        .await
        .map_err(|_| PlaceFileError::Unreadable)?
        .map_err(|_| PlaceFileError::Unreadable)?;
        Ok(Sha256Hex::from_digest(&digest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_relative_paths_land_inside_the_folder() {
        let folder = Path::new("/agents/a/folder");
        assert_eq!(
            inside(folder, "sandbox-output/r/out.txt"),
            Some(folder.join("sandbox-output/r/out.txt"))
        );
        for bad in ["", "/etc/passwd", "../x", "a/../../x", "a\\b", "./a"] {
            assert_eq!(inside(folder, bad), None, "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_the_folder_is_not_followed() {
        let dir = crate::test_support::TempDir::new("links");
        let root = dir.path().join("folder");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).expect("folder");
        std::fs::create_dir_all(&outside).expect("outside");
        std::os::unix::fs::symlink(&outside, root.join("out")).expect("link");
        assert!(make_folders(&root, &root.join("out/x.txt")).is_err());
        std::os::unix::fs::symlink(outside.join("f"), root.join("f.txt")).expect("file link");
        assert!(make_folders(&root, &root.join("f.txt")).is_err());
        assert_eq!(
            make_folders(&root, &root.join("a/b/x.txt")).expect("plain folders"),
            root.join("a/b")
        );
        assert!(root.join("a/b").is_dir());
    }

    #[test]
    fn file_service_refusals_map_to_the_sandboxs() {
        assert_eq!(
            place_error(FileError::NotGranted),
            PlaceFileError::NotGranted
        );
        assert_eq!(place_error(FileError::NotFound), PlaceFileError::NotFound);
        assert_eq!(
            place_error(FileError::PlaceNotFound),
            PlaceFileError::NotFound
        );
        assert_eq!(
            place_error(FileError::PathEscapesPlace),
            PlaceFileError::NotFound
        );
        assert_eq!(
            place_error(FileError::TooLarge { limit_bytes: 1 }),
            PlaceFileError::TooLarge
        );
        assert_eq!(
            place_error(FileError::WriteFailed),
            PlaceFileError::Unreadable
        );
    }
}
