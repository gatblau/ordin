//! Grant hand-off and grant-checked reads (FileService.md Internal Logic 3 and 12).

use ordin_agent_folders::WorkspaceGrant;
use ordin_rules::GrantedPlace;
use ordin_types::domain::places::{FolderGrant, Grantee};
use ordin_types::{AgentInstanceId, PlaceId, WorkspaceId};

use crate::error::FileError;
use crate::log;
use crate::paths::{self, normalise_file};
use crate::service::{lock, FileService};
use crate::versions::blocking;
use crate::READ_LIMIT_BYTES;

impl FileService {
    /// Hands the grants of one workspace to the Agent Folder Manager and the Rules Engine (B7
    /// contract D-7), and keeps them for `read_for_agent`.
    // spec: ordin-desktop/components/FileService#SA-RECON-02
    pub async fn publish_grants(
        &self,
        workspace_id: WorkspaceId,
        grants: Vec<FolderGrant>,
    ) -> Result<(), FileError> {
        // 3.1 and 3.2
        let mut handed = Vec::with_capacity(grants.len());
        let mut granted_places = Vec::with_capacity(grants.len());
        for grant in grants {
            let place = match self.place_row(grant.place_id).await {
                Ok(place) => place,
                Err(FileError::PlaceNotFound) => {
                    log::grant_place_missing(workspace_id, grant.place_id);
                    continue;
                }
                Err(other) => return Err(other),
            };
            granted_places.push(GrantedPlace {
                grantee: grant.grantee,
                place_id: place.id,
                root: place.root.clone(),
                access: grant.access,
            });
            handed.push(WorkspaceGrant {
                grant,
                place_path: place.root,
                place_kind: place.kind,
            });
        }
        let count = handed.len();
        // 3.3: the Rules Engine is told even when the first call fails, so enforcement is never
        // more permissive than the stored grants.
        let first = self
            .agent_folders
            .set_workspace_grants(workspace_id, handed.clone())
            .await;
        self.rules.replace_grants(workspace_id, granted_places);
        // 12.1
        lock(&self.granted).insert(workspace_id, handed);
        // `FileError::GrantHandoffFailed` has no field for the retry class of the Agent Folder
        // Manager's error, so the error is returned as retryable in both classes.
        if first.is_err() {
            return Err(FileError::GrantHandoffFailed);
        }
        // 3.4
        self.record(
            "grants_published",
            Some(workspace_id),
            None,
            "Folder access was handed to the agents of a workspace.",
            serde_json::json!({ "workspace_id": workspace_id.to_string(), "count": count }),
        )
        .await;
        log::grants_published(workspace_id, count);
        Ok(())
    }

    /// Reads one file for an agent instance, only when the workspace's last published grants give
    /// that instance a grant on `place_id` (Internal Logic 12).
    pub async fn read_for_agent(
        &self,
        agent_instance_id: AgentInstanceId,
        workspace_id: WorkspaceId,
        place_id: PlaceId,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, FileError> {
        let allowed = lock(&self.granted)
            .get(&workspace_id)
            .is_some_and(|grants| {
                grants.iter().any(|g| {
                g.grant.place_id == place_id
                    && matches!(
                        g.grant.grantee,
                        Grantee::AgentInstance { agent_instance_id: id } if id == agent_instance_id
                    )
            })
            });
        if !allowed {
            return Err(FileError::NotGranted);
        }
        let path = normalise_file(relative_path)?;
        let limit = max_bytes.min(READ_LIMIT_BYTES);
        let place = self.place_row(place_id).await?;
        let abs = paths::confine(&place.root, &path)?;
        blocking(move || -> Result<Vec<u8>, FileError> {
            let meta = std::fs::metadata(&abs)
                .ok()
                .filter(|m| m.is_file())
                .ok_or(FileError::NotFound)?;
            if meta.len() > limit {
                return Err(FileError::TooLarge { limit_bytes: limit });
            }
            std::fs::read(&abs).map_err(|_| FileError::NotFound)
        })
        .await?
    }
}
