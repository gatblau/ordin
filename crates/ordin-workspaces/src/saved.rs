//! Files saved by the Channel Gateway into a workspace's own folder (WorkspaceService.md Internal
//! Logic 10a.1 and 10a.2; SA-CLOSE-02).

use std::path::Path;

use bytes::Bytes;
use ordin_files::{FileVersionOrigin, WriteVersion};
use ordin_types::domain::server::ChannelKind;
use ordin_types::{RunId, WorkspaceId};

use crate::error::{file_failed, WorkspaceError};
use crate::service::WorkspaceService;
use crate::store::to_text;
use crate::types::{InboxOrigin, PlaceFileRef};
use crate::SAVED_FILE_MAX_BYTES;

/// 1–255 characters; no `/`, `\`, `:` or control character; not `.` or `..`.
pub(crate) fn check_file_name(name: &str) -> Result<String, WorkspaceError> {
    let trimmed = name.trim();
    let chars = trimmed.chars().count();
    if chars == 0
        || chars > 255
        || trimmed == "."
        || trimmed == ".."
        || trimmed
            .chars()
            .any(|c| c == '/' || c == '\\' || c == ':' || c.is_control())
    {
        return Err(WorkspaceError::FileNameInvalid);
    }
    Ok(trimmed.to_owned())
}

/// `<stem> (n).<ext>` for the n-th numbered name.
fn numbered(name: &str, n: u32) -> String {
    let path = Path::new(name);
    match (
        path.file_stem().and_then(|s| s.to_str()),
        path.extension().and_then(|e| e.to_str()),
    ) {
        (Some(stem), Some(ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
        _ => format!("{name} ({n})"),
    }
}

/// Where a saved file goes.
enum SaveTarget {
    /// A folder of the workspace's own folder.
    Own(&'static str),
    /// Ordin's saved files: the results folder's root in a team workspace (WS-TW-2), else `Results/`
    /// of the own folder.
    Generated,
}

impl WorkspaceService {
    async fn save_in(
        &self,
        ws: WorkspaceId,
        target: SaveTarget,
        name: &str,
        bytes: Bytes,
        origin: FileVersionOrigin,
    ) -> Result<(PlaceFileRef, String), WorkspaceError> {
        let workspace = self.not_in_bin(ws).await?;
        let name = check_file_name(name)?;
        if bytes.len() as u64 > SAVED_FILE_MAX_BYTES {
            return Err(WorkspaceError::FileTooLarge);
        }
        // spec: ordin-desktop/components/WorkspaceService#WS-TW-2
        let (place_id, folder, shown) = match target {
            SaveTarget::Own(folder) => (workspace.own_folder_place_id, Some(folder), folder),
            SaveTarget::Generated => match self.live_results_place(ws).await? {
                Some(results) => (results, None, crate::TEAM_RESULTS_NAME),
                None => (workspace.own_folder_place_id, Some("Results"), "Results"),
            },
        };
        let place = self.files.place(place_id).await.map_err(file_failed)?;
        let root = std::path::PathBuf::from(&place.path);
        let dir = match folder {
            Some(folder) => root.join(folder),
            None => root,
        };
        let mut chosen = None;
        for n in 1..=99u32 {
            let candidate = if n == 1 {
                name.clone()
            } else {
                numbered(&name, n)
            };
            let taken = dir.join(&candidate);
            let exists = tokio::task::spawn_blocking(move || taken.exists())
                .await
                .map_err(|_| WorkspaceError::FilesFailed)?;
            if !exists {
                chosen = Some(candidate);
                break;
            }
        }
        let saved = chosen.ok_or(WorkspaceError::FileNameInvalid)?;
        let relative_path = match folder {
            Some(folder) => format!("{folder}/{saved}"),
            None => saved.clone(),
        };
        self.files
            .write_version(WriteVersion {
                place_id,
                path: relative_path.clone(),
                bytes: bytes.to_vec(),
                origin,
                expected_hash: None,
            })
            .await
            .map_err(file_failed)?;
        let reference = PlaceFileRef {
            place_id,
            display_path: format!("{} › {shown} › {saved}", workspace.name),
            relative_path,
        };
        Ok((reference, saved))
    }

    /// Internal Logic 10a.1: saves a file the person sent in a chat into `Inbox/`.
    pub async fn save_to_inbox(
        &self,
        ws: WorkspaceId,
        name: &str,
        bytes: Bytes,
        origin: InboxOrigin,
    ) -> Result<PlaceFileRef, WorkspaceError> {
        let (reference, _) = self
            .save_in(
                ws,
                SaveTarget::Own("Inbox"),
                name,
                bytes,
                FileVersionOrigin::Person,
            )
            .await?;
        let InboxOrigin::Chat { channel } = origin;
        self.record(
            "inbox_file_saved",
            Some(ws),
            "A file from a chat was saved in a workspace.",
            serde_json::json!({ "workspace_id": ws, "channel": channel_text(channel) }),
        )
        .await;
        self.changed(ws).await;
        Ok(reference)
    }

    /// Internal Logic 10a.2: saves a file made for a chat reply by run `by` into `Results/`, or into
    /// the results folder's root in a team workspace (WS-TW-2), where WS-TW-3 records it with its run.
    pub async fn save_generated(
        &self,
        ws: WorkspaceId,
        name: &str,
        bytes: Bytes,
        by: RunId,
    ) -> Result<PlaceFileRef, WorkspaceError> {
        let origin = FileVersionOrigin::Run {
            run_id: by,
            tool: "channel_gateway".to_owned(),
        };
        let (reference, _) = self
            .save_in(ws, SaveTarget::Generated, name, bytes, origin)
            .await?;
        self.record(
            "generated_file_saved",
            Some(ws),
            "A file made for a chat reply was saved in a workspace.",
            serde_json::json!({ "workspace_id": ws, "run_id": by }),
        )
        .await;
        self.changed(ws).await;
        Ok(reference)
    }
}

fn channel_text(channel: ChannelKind) -> String {
    to_text(&channel)
}
