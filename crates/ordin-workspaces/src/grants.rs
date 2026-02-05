//! Folder grants (WorkspaceService.md Internal Logic 8.1 and 8.6): a grant in one workspace gives
//! nothing in another, and stored and enforced grants never differ.

use ordin_files::PlaceKind;
use ordin_types::domain::places::{FolderAccess, FolderGrant, Grantee, SourceLinkKind};
use ordin_types::time::Timestamp;
use ordin_types::{FolderGrantId, PlaceId, WorkspaceId};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::error::{file_failed, is_unique_violation, WorkspaceError};
use crate::log;
use crate::mail::{by_table, Table};
use crate::service::WorkspaceService;
use crate::store::{self, from_text, to_text};

pub(crate) fn grantee_key(grantee: &Grantee) -> String {
    match grantee {
        Grantee::AgentInstance { agent_instance_id } => format!("agent:{agent_instance_id}"),
        Grantee::Crew { crew_charter_id } => format!("crew:{crew_charter_id}"),
    }
}

fn grantee_kind(grantee: &Grantee) -> &'static str {
    match grantee {
        Grantee::AgentInstance { .. } => "agent_instance",
        Grantee::Crew { .. } => "crew",
    }
}

pub(crate) fn grant_row(row: &SqliteRow) -> Result<FolderGrant, WorkspaceError> {
    let grantee: String = row.try_get("grantee")?;
    Ok(FolderGrant {
        id: store::id_of(row, "id")?,
        workspace_id: store::id_of(row, "workspace_id")?,
        grantee: serde_json::from_str(&grantee)
            .map_err(|_| WorkspaceError::DatastoreUnavailable)?,
        place_id: store::id_of(row, "place_id")?,
        access: from_text::<FolderAccess>(&row.try_get::<String, _>("access")?)?,
        granted_at: store::ts_req(row, "granted_at")?,
    })
}

impl WorkspaceService {
    pub(crate) async fn delete_grants_on(
        &self,
        ws: WorkspaceId,
        place: Option<PlaceId>,
    ) -> Result<(), WorkspaceError> {
        if let Some(place) = place {
            sqlx::query("DELETE FROM folder_grants WHERE workspace_id = ?1 AND place_id = ?2")
                .bind(ws.to_string())
                .bind(place.to_string())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Writes a stored grant back as it was (a rollback).
    pub(crate) async fn restore_grant(&self, grant: &FolderGrant) -> Result<(), WorkspaceError> {
        sqlx::query(
            "INSERT OR REPLACE INTO folder_grants (id, workspace_id, grantee, grantee_key, place_id, access, granted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(grant.id.to_string())
        .bind(grant.workspace_id.to_string())
        .bind(serde_json::to_string(&grant.grantee).unwrap_or_default())
        .bind(grantee_key(&grant.grantee))
        .bind(grant.place_id.to_string())
        .bind(to_text(&grant.access))
        .bind(grant.granted_at.to_wire())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Internal Logic 8.1. `access = None` is "can't use": the grant is removed.
    pub async fn set_grant(
        &self,
        id: WorkspaceId,
        grantee: Grantee,
        place_id: PlaceId,
        access: Option<FolderAccess>,
    ) -> Result<Option<FolderGrant>, WorkspaceError> {
        self.not_in_bin(id).await?;
        let member = sqlx::query(
            "SELECT count(*) AS n FROM workspace_places WHERE workspace_id = ?1 AND place_id = ?2",
        )
        .bind(id.to_string())
        .bind(place_id.to_string())
        .fetch_one(&self.pool)
        .await?
        .try_get::<i64, _>("n")?;
        if member == 0 {
            return Err(WorkspaceError::Invalid("GRANT_PLACE_NOT_IN_WORKSPACE"));
        }
        let place = self.files.place(place_id).await.map_err(file_failed)?;
        if matches!(
            place.kind,
            PlaceKind::AgentFolder | PlaceKind::CodingTaskWorktree
        ) {
            return Err(WorkspaceError::PlaceRefused {
                cause: "hidden_place",
            });
        }
        if access == Some(FolderAccess::ReadAndChange) && self.is_jira_place(id, place_id).await? {
            return Err(WorkspaceError::JiraReadOnly);
        }
        let key = grantee_key(&grantee);
        let existing = sqlx::query(
            "SELECT * FROM folder_grants WHERE workspace_id = ?1 AND grantee_key = ?2 AND place_id = ?3",
        )
        .bind(id.to_string())
        .bind(&key)
        .bind(place_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let previous = existing.as_ref().map(grant_row).transpose()?;
        let now = Timestamp::now();
        let stored = match (&previous, access) {
            (Some(grant), Some(access)) => {
                sqlx::query("UPDATE folder_grants SET access = ?2, granted_at = ?3 WHERE id = ?1")
                    .bind(grant.id.to_string())
                    .bind(to_text(&access))
                    .bind(now.to_wire())
                    .execute(&self.pool)
                    .await?;
                Some(FolderGrant {
                    access,
                    granted_at: now,
                    ..grant.clone()
                })
            }
            (None, Some(access)) => {
                let grant = FolderGrant {
                    id: FolderGrantId::new(),
                    workspace_id: id,
                    grantee,
                    place_id,
                    access,
                    granted_at: now,
                };
                self.insert_grant(&grant, &key).await?;
                Some(grant)
            }
            (Some(grant), None) => {
                sqlx::query("DELETE FROM folder_grants WHERE id = ?1")
                    .bind(grant.id.to_string())
                    .execute(&self.pool)
                    .await?;
                None
            }
            (None, None) => None,
        };
        // Handed to enforcement before the change is reported; a failure rolls the row back.
        if let Err(error) = self.publish(id).await {
            match (&previous, &stored) {
                (Some(old), _) => {
                    if let Err(error) = self.restore_grant(old).await {
                        crate::log::rollback_failed("restore_grant", error.code());
                    }
                }
                (None, Some(new)) => {
                    if let Err(error) = sqlx::query("DELETE FROM folder_grants WHERE id = ?1")
                        .bind(new.id.to_string())
                        .execute(&self.pool)
                        .await
                    {
                        let error = WorkspaceError::from(error);
                        crate::log::rollback_failed("delete_grant", error.code());
                    }
                }
                (None, None) => {}
            }
            // Enforcement may have taken the new list before it failed: tell it the stored one again.
            if self.publish(id).await.is_err() {
                self.mark_unpublished(id);
            }
            return Err(error);
        }
        let access_text: Option<String> = access.map(|a| to_text(&a));
        let logged: &'static str = match access {
            Some(FolderAccess::Read) => "read",
            Some(FolderAccess::ReadAndChange) => "read_and_change",
            None => "none",
        };
        log::grant_set(id, place_id, grantee_kind(&grantee), logged);
        self.record(
            "grant_set",
            Some(id),
            "Folder access was changed in a workspace.",
            serde_json::json!({
                "workspace_id": id,
                "place_id": place_id,
                "grantee_kind": grantee_kind(&grantee),
                "access": access_text,
            }),
        )
        .await;
        self.changed(id).await;
        Ok(stored)
    }

    pub(crate) async fn insert_grant(
        &self,
        grant: &FolderGrant,
        key: &str,
    ) -> Result<(), WorkspaceError> {
        sqlx::query(
            "INSERT INTO folder_grants (id, workspace_id, grantee, grantee_key, place_id, access, granted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(grant.id.to_string())
        .bind(grant.workspace_id.to_string())
        .bind(serde_json::to_string(&grant.grantee).unwrap_or_default())
        .bind(key)
        .bind(grant.place_id.to_string())
        .bind(to_text(&grant.access))
        .bind(grant.granted_at.to_wire())
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                WorkspaceError::Invalid("GRANT_DUPLICATE")
            } else {
                error.into()
            }
        })?;
        Ok(())
    }

    async fn is_jira_place(&self, ws: WorkspaceId, place: PlaceId) -> Result<bool, WorkspaceError> {
        let row = sqlx::query(
            "SELECT count(*) AS n FROM source_links WHERE workspace_id = ?1 AND place_id = ?2 AND kind = ?3",
        )
        .bind(ws.to_string())
        .bind(place.to_string())
        .bind(to_text(&SourceLinkKind::JiraIssue))
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get::<i64, _>("n")? > 0)
    }

    pub async fn grants(&self, id: WorkspaceId) -> Result<Vec<FolderGrant>, WorkspaceError> {
        self.workspace(id).await?;
        self.grants_of(id).await
    }

    /// Internal Logic 8.6: removes a grantee's grants in every workspace (agent instance removed,
    /// 04:61) and, for an agent instance, its email, Telegram and Slack grants; each affected
    /// workspace's grants are republished. Returns the number of grants removed.
    pub async fn revoke_grantee(&self, grantee: Grantee) -> Result<u32, WorkspaceError> {
        let key = grantee_key(&grantee);
        let mut affected: std::collections::BTreeSet<WorkspaceId> =
            std::collections::BTreeSet::new();
        let mut removed: u64 = 0;
        let rows =
            sqlx::query("SELECT DISTINCT workspace_id FROM folder_grants WHERE grantee_key = ?1")
                .bind(&key)
                .fetch_all(&self.pool)
                .await?;
        for row in &rows {
            affected.insert(store::id_of::<WorkspaceId>(row, "workspace_id")?);
        }
        removed += sqlx::query("DELETE FROM folder_grants WHERE grantee_key = ?1")
            .bind(&key)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if let Grantee::AgentInstance { agent_instance_id } = grantee {
            for table in [Table::Email, Table::Telegram, Table::Slack] {
                let rows = sqlx::query(by_table!(
                    table,
                    "SELECT DISTINCT workspace_id FROM ",
                    " WHERE agent_instance_id = ?1"
                ))
                .bind(agent_instance_id.to_string())
                .fetch_all(&self.pool)
                .await?;
                for row in &rows {
                    affected.insert(store::id_of(row, "workspace_id")?);
                }
                removed += sqlx::query(by_table!(
                    table,
                    "DELETE FROM ",
                    " WHERE agent_instance_id = ?1"
                ))
                .bind(agent_instance_id.to_string())
                .execute(&self.pool)
                .await?
                .rows_affected();
            }
        }
        let mut failed = false;
        for ws in &affected {
            let state = self.workspace(*ws).await.map(|w| w.state);
            if matches!(
                state,
                Ok(ordin_types::domain::places::WorkspaceState::InBin) | Err(_)
            ) {
                continue;
            }
            if self.publish(*ws).await.is_err() {
                log::grants_publish_failed(*ws);
                self.mark_unpublished(*ws);
                failed = true;
            }
        }
        for ws in &affected {
            self.record(
                "grant_set",
                Some(*ws),
                "Folder access was changed in a workspace.",
                serde_json::json!({ "workspace_id": ws, "grantee_kind": grantee_kind(&grantee), "access": null }),
            )
            .await;
            self.changed(*ws).await;
        }
        if failed {
            return Err(WorkspaceError::GrantPublishFailed);
        }
        Ok(u32::try_from(removed).unwrap_or(u32::MAX))
    }
}
