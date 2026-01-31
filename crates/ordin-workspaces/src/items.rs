//! Working-copy items: their states, what the File Service's changes do to them, and the reads the
//! Push Service and the Knowledge Service make (WorkspaceService.md Internal Logic 6).

use ordin_files::{PlaceChange, PlaceChangeKind};
use ordin_types::domain::places::{SourceLink, SourceLinkKind, WorkingCopyItem, WorkingCopyState};
use ordin_types::time::Timestamp;
use ordin_types::{SourceLinkId, WorkingCopyItemId, WorkspaceId};
use sqlx::Row;
use tokio::sync::broadcast;

use crate::error::WorkspaceError;
use crate::log;
use crate::service::WorkspaceService;
use crate::store::{self, to_text};
use crate::types::{OriginChanged, OriginVersion, WorkingCopyEntry};

/// 2C's WorkingCopyItem transition table with the two additions of HLD 04:85 (SA-CLOSE-01).
pub(crate) fn transition_allowed(from: WorkingCopyState, to: WorkingCopyState) -> bool {
    use WorkingCopyState::*;
    matches!(
        (from, to),
        (Read, Changed)
            | (Changed, AwaitingPush)
            | (AwaitingPush, Pushed)
            | (AwaitingPush, OriginChanged)
            | (OriginChanged, Read)
            | (Pushed, Read)
            | (AwaitingPush, Changed)
    )
}

impl WorkspaceService {
    pub async fn working_copy_items(
        &self,
        link: SourceLinkId,
    ) -> Result<Vec<WorkingCopyItem>, WorkspaceError> {
        store::load_link(&self.pool, link).await?;
        let rows = sqlx::query(
            "SELECT * FROM working_copy_items WHERE source_link_id = ?1 ORDER BY origin_path",
        )
        .bind(link.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(store::item_row).collect()
    }

    pub async fn working_copy_item(
        &self,
        id: WorkingCopyItemId,
    ) -> Result<WorkingCopyItem, WorkspaceError> {
        store::load_item(&self.pool, id).await
    }

    /// Internal Logic 6.5.
    pub async fn working_copy_entry(
        &self,
        id: WorkingCopyItemId,
    ) -> Result<WorkingCopyEntry, WorkspaceError> {
        let item = store::load_item(&self.pool, id).await?;
        let row = sqlx::query("SELECT * FROM working_copy_items WHERE id = ?1")
            .bind(id.to_string())
            .fetch_one(&self.pool)
            .await?;
        let git_branch: Option<String> = row.try_get("git_branch")?;
        let link = store::load_link(&self.pool, item.source_link_id).await?;
        let place_id = link
            .place_id
            .ok_or(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"))?;
        let repository = if link.kind == SourceLinkKind::GitRepositoryFolder {
            self.linked_repository(&link).await
        } else {
            None
        };
        Ok(WorkingCopyEntry {
            workspace_id: link.workspace_id,
            link_kind: link.kind,
            origin_location: link.origin_location,
            display_name: link.display_name,
            place_id,
            relative_path: item.origin_path.clone(),
            git_branch,
            repository,
            item,
        })
    }

    /// Internal Logic 6.1: validates the transition; `origin_version` is required for `pushed`.
    pub async fn set_working_copy_state(
        &self,
        id: WorkingCopyItemId,
        state: WorkingCopyState,
        origin_version: Option<String>,
    ) -> Result<WorkingCopyItem, WorkspaceError> {
        let item = store::load_item(&self.pool, id).await?;
        if !transition_allowed(item.state, state) {
            return Err(WorkspaceError::WorkingCopyTransitionInvalid {
                from: item.state,
                to: state,
            });
        }
        if state == WorkingCopyState::Pushed {
            let version = origin_version.ok_or(WorkspaceError::Invalid("WCI_VERSION_INVALID"))?;
            store::check_item(&item.origin_path, &version)?;
            sqlx::query("UPDATE working_copy_items SET state = 'pushed', origin_version_read = ?2 WHERE id = ?1")
                .bind(id.to_string())
                .bind(version)
                .execute(&self.pool)
                .await?;
        } else {
            sqlx::query("UPDATE working_copy_items SET state = ?2 WHERE id = ?1")
                .bind(id.to_string())
                .bind(to_text(&state))
                .execute(&self.pool)
                .await?;
        }
        store::load_item(&self.pool, id).await
    }

    /// Internal Logic 6.6: the origin version last recorded; `None` when no such item exists.
    pub async fn origin_version(
        &self,
        workspace_id: WorkspaceId,
        link_kind: SourceLinkKind,
        origin_path: &str,
    ) -> Result<Option<OriginVersion>, WorkspaceError> {
        if !matches!(
            link_kind,
            SourceLinkKind::SharepointLocation
                | SourceLinkKind::ConfluenceLocation
                | SourceLinkKind::JiraIssue
        ) {
            return Err(WorkspaceError::Invalid("SRC_KIND_MISMATCH"));
        }
        let row = sqlx::query(
            "SELECT i.origin_version_read AS version, i.origin_changed_by AS by, i.origin_changed_at AS at \
             FROM working_copy_items i JOIN source_links l ON l.id = i.source_link_id \
             WHERE l.workspace_id = ?1 AND l.kind = ?2 AND l.state = 'active' AND i.origin_path = ?3",
        )
        .bind(workspace_id.to_string())
        .bind(to_text(&link_kind))
        .bind(origin_path)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(OriginVersion {
            version: row.try_get("version")?,
            changed_by: row.try_get("by")?,
            changed_at: store::ts_opt(&row, "at")?,
        }))
    }

    /// Internal Logic 10a.3: the latest origin change of each item since `since`, newest first, at
    /// most 1,000; `removed` is always false here.
    pub async fn origin_changes_since(
        &self,
        since: Timestamp,
    ) -> Result<Vec<OriginChanged>, WorkspaceError> {
        let rows = sqlx::query(
            "SELECT l.workspace_id AS workspace_id, l.id AS link_id, l.kind AS kind, i.origin_path AS path, \
             i.origin_version_read AS version, i.origin_changed_by AS by, i.origin_changed_at AS at \
             FROM working_copy_items i JOIN source_links l ON l.id = i.source_link_id \
             JOIN workspaces w ON w.id = l.workspace_id \
             WHERE i.origin_changed_at > ?1 AND l.state = 'active' AND w.state != 'in_bin' \
             ORDER BY i.origin_changed_at DESC, i.id LIMIT 1000",
        )
        .bind(since.to_wire())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(OriginChanged {
                    workspace_id: store::id_of(row, "workspace_id")?,
                    source_link_id: store::id_of(row, "link_id")?,
                    link_kind: store::from_text(&row.try_get::<String, _>("kind")?)?,
                    origin_path: row.try_get("path")?,
                    version: OriginVersion {
                        version: row.try_get("version")?,
                        changed_by: row.try_get("by")?,
                        changed_at: store::ts_opt(row, "at")?,
                    },
                    removed: false,
                })
            })
            .collect()
    }

    // ---- 6.2 and 6.7: what the File Service's changes do

    pub(crate) async fn follow_place_changes(&self, mut changes: broadcast::Receiver<PlaceChange>) {
        loop {
            match changes.recv().await {
                Ok(change) => self.handle_place_change(change).await,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    log::background_failed("place_changes", "lagged");
                }
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    }

    /// One File Service change: the working-copy item it concerns, a git or local folder item, a
    /// Jira item (never changed), and the debounced Confluence folder sync.
    pub(crate) async fn handle_place_change(&self, change: PlaceChange) {
        self.schedule_folder_syncs(&change).await;
        let rows =
            sqlx::query("SELECT * FROM source_links WHERE place_id = ?1 AND state = 'active'")
                .bind(change.place_id.to_string())
                .fetch_all(&self.pool)
                .await;
        let Ok(rows) = rows else { return };
        for row in &rows {
            let Ok(link) = store::link_row(row) else {
                continue;
            };
            let outcome = match link.kind {
                SourceLinkKind::JiraIssue => {
                    // §J.3: read only; the Push Service never proposes a push to Jira.
                    log::jira_item_local_change(link.id);
                    Ok(())
                }
                SourceLinkKind::LocalFolder | SourceLinkKind::GitRepositoryFolder => {
                    self.folder_change(&link, &change).await
                }
                SourceLinkKind::SharepointLocation | SourceLinkKind::ConfluenceLocation => {
                    self.existing_item_change(&link, &change).await.map(|_| ())
                }
                SourceLinkKind::GitRemote => Ok(()),
            };
            if let Err(error) = outcome {
                log::background_failed("place_change", error.code());
            }
        }
    }

    /// Internal Logic 6.2: a change to a path with an item. Returns whether an item exists.
    async fn existing_item_change(
        &self,
        link: &SourceLink,
        change: &PlaceChange,
    ) -> Result<bool, WorkspaceError> {
        if !matches!(
            change.change,
            PlaceChangeKind::Created | PlaceChangeKind::Modified
        ) {
            return Ok(false);
        }
        let Some(item) = store::item_at(&self.pool, link.id, &change.path).await? else {
            return Ok(false);
        };
        // `pushed` first applies `pushed → read` (6.3), then `read → changed`.
        let moves_to_changed = matches!(
            item.state,
            WorkingCopyState::Read | WorkingCopyState::Pushed
        );
        if moves_to_changed {
            sqlx::query("UPDATE working_copy_items SET state = 'changed', local_version = local_version + 1 WHERE id = ?1")
                .bind(item.id.to_string())
                .execute(&self.pool)
                .await?;
            let _ = self.wc_tx.send(item.id);
        } else {
            sqlx::query(
                "UPDATE working_copy_items SET local_version = local_version + 1 WHERE id = ?1",
            )
            .bind(item.id.to_string())
            .execute(&self.pool)
            .await?;
        }
        Ok(true)
    }

    /// Internal Logic 6.7: a local or git folder is its own working copy.
    async fn folder_change(
        &self,
        link: &SourceLink,
        change: &PlaceChange,
    ) -> Result<(), WorkspaceError> {
        if self.existing_item_change(link, change).await? {
            return Ok(());
        }
        if !change.by_ordin
            || !matches!(
                change.change,
                PlaceChangeKind::Created | PlaceChangeKind::Modified
            )
        {
            return Ok(());
        }
        let (version, branch) = if link.kind == SourceLinkKind::LocalFolder {
            let before = change.version.map_or(0, |v| v.saturating_sub(1));
            (format!("fver:{before}"), None)
        } else {
            let place = self
                .files
                .place(
                    link.place_id
                        .ok_or(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"))?,
                )
                .await
                .map_err(crate::error::file_failed)?;
            match crate::repos::git_origin(std::path::Path::new(&place.path)) {
                Some((branch, commit)) => (commit, Some(branch)),
                None => {
                    log::working_copy_git_unreadable(link.id);
                    return Ok(());
                }
            }
        };
        match store::insert_item(
            &self.pool,
            link.id,
            &change.path,
            &version,
            2,
            WorkingCopyState::Changed,
            branch.as_deref(),
        )
        .await
        {
            Ok(item) => {
                let _ = self.wc_tx.send(item.id);
                Ok(())
            }
            Err(WorkspaceError::Invalid("WCI_DUPLICATE")) => Ok(()),
            Err(error) => Err(error),
        }
    }
}
