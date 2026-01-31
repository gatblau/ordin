//! Origin checks (WorkspaceService.md Internal Logic 7) and what an origin change does to a
//! working-copy item (Internal Logic 6.4).

use std::sync::atomic::{AtomicBool, Ordering};

use ordin_types::domain::places::{SourceLink, SourceLinkKind, SourceLinkState, WorkingCopyState};
use ordin_types::time::Timestamp;
use ordin_types::{CodedError, SourceLinkId, WorkspaceId};

use crate::error::{file_failed, WorkspaceError};
use crate::links::map_adapter;
use crate::log;
use crate::remote::ItemRef;
use crate::service::{lock, WorkspaceService};
use crate::store;
use crate::types::{OriginChanged, OriginVersion};

/// An origin change, whichever adapter reported it.
struct Change {
    origin_path: String,
    new_version: String,
    changed_by: Option<String>,
    changed_at: Timestamp,
    removed: bool,
}

/// Which adapters answered "sign in" during one trigger; their later links are skipped (7.2).
#[derive(Default)]
struct SignInStops {
    sharepoint: AtomicBool,
    confluence: AtomicBool,
    jira: AtomicBool,
}

/// What one link's check came to.
enum Checked {
    Changes(u32),
    Failed,
    Skipped,
}

impl WorkspaceService {
    /// Internal Logic 7: the origin check of one workspace's remote links, `CHECK_CONCURRENCY` links at
    /// a time across workspaces, then the Confluence linked-folder syncs. Returns the number of changes
    /// found and the links whose check failed. A workspace that is not active has nothing checked.
    pub(crate) async fn check_workspace(&self, id: WorkspaceId) -> (u32, Vec<SourceLinkId>) {
        if self.active_workspace_state(id).await
            != Some(ordin_types::domain::places::WorkspaceState::Active)
        {
            return (0, Vec::new());
        }
        let Ok(links) = self.links_of(id).await else {
            return (0, Vec::new());
        };
        let remote: Vec<SourceLink> = links
            .into_iter()
            .filter(|l| l.state == SourceLinkState::Active && crate::service::is_remote(l.kind))
            .collect();
        let stops = SignInStops::default();
        let results =
            futures::future::join_all(remote.iter().map(|link| self.check_link(link, &stops)))
                .await;
        let mut changes = 0;
        let mut failed = Vec::new();
        for (link, result) in remote.iter().zip(results) {
            match result {
                Checked::Changes(n) => changes += n,
                Checked::Failed => failed.push(link.id),
                Checked::Skipped => {}
            }
        }
        // 7.3: Confluence linked-folder publishing.
        if let Ok(ids) = self.confluence_link_ids(id).await {
            for link in ids {
                self.sync_confluence_link(link).await;
            }
        }
        let failed_count = u32::try_from(failed.len()).unwrap_or(u32::MAX);
        log::origin_check_done(id, changes, failed_count);
        self.record(
            "origin_check_done",
            Some(id),
            "The origins of a workspace's linked locations were checked.",
            serde_json::json!({ "workspace_id": id, "changes": changes, "failed": failed_count }),
        )
        .await;
        (changes, failed)
    }

    async fn check_link(&self, link: &SourceLink, stops: &SignInStops) -> Checked {
        let stop = match link.kind {
            SourceLinkKind::SharepointLocation => &stops.sharepoint,
            SourceLinkKind::ConfluenceLocation => &stops.confluence,
            _ => &stops.jira,
        };
        if stop.load(Ordering::SeqCst) {
            return Checked::Skipped;
        }
        if !lock(&self.checking).insert(link.id) {
            return Checked::Skipped;
        }
        let permit = self.check_slots.clone().acquire_owned().await;
        let outcome = self.check_one(link).await;
        if outcome.is_ok() {
            // Internal Logic 4.3: items left unread or failed are tried again at this check.
            self.retry_unread(link).await;
        }
        drop(permit);
        lock(&self.checking).remove(&link.id);
        match outcome {
            Ok(changes) => Checked::Changes(changes),
            Err(error) => {
                log::origin_check_failed(link.id, error.code());
                if matches!(error, WorkspaceError::SourceSignInNeeded) {
                    stop.store(true, Ordering::SeqCst);
                }
                Checked::Failed
            }
        }
    }

    async fn check_one(&self, link: &SourceLink) -> Result<u32, WorkspaceError> {
        let changes: Vec<Change> = match link.kind {
            SourceLinkKind::SharepointLocation => match self.sharepoint.check_changes(link).await {
                Ok(found) => found
                    .into_iter()
                    .map(|c| Change {
                        origin_path: c.origin_path,
                        new_version: c.new_version,
                        changed_by: c.changed_by,
                        changed_at: c.changed_at,
                        removed: c.removed,
                    })
                    .collect(),
                Err(error) if error.code() == "SP_NOT_TRACKED" => {
                    self.sharepoint
                        .track_link(link)
                        .await
                        .map_err(|e| map_adapter(&e))?;
                    Vec::new()
                }
                Err(error) => return Err(map_adapter(&error)),
            },
            SourceLinkKind::ConfluenceLocation => {
                let confluence = self
                    .confluence
                    .as_ref()
                    .ok_or(WorkspaceError::ConfluenceUnavailable)?;
                match confluence.check_changes(link).await {
                    Ok(found) => found
                        .into_iter()
                        .map(|c| Change {
                            origin_path: c.origin_path,
                            new_version: c.new_version,
                            changed_by: c.changed_by,
                            changed_at: c.changed_at,
                            removed: c.removed,
                        })
                        .collect(),
                    Err(error) if error.code() == "CONF_NOT_TRACKED" => {
                        confluence
                            .track_link(link)
                            .await
                            .map_err(|e| map_adapter(&e))?;
                        Vec::new()
                    }
                    Err(error) => return Err(map_adapter(&error)),
                }
            }
            SourceLinkKind::JiraIssue => return self.jira_check(link).await,
            _ => Vec::new(),
        };
        let mut counted = 0;
        for change in changes {
            if self.apply_change(link, change).await? {
                counted += 1;
            }
        }
        Ok(counted)
    }

    /// Internal Logic 6.4. Returns true when the change counts for `OpenReport.origin_changes`.
    async fn apply_change(
        &self,
        link: &SourceLink,
        change: Change,
    ) -> Result<bool, WorkspaceError> {
        let existing = store::item_at(&self.pool, link.id, &change.origin_path).await?;
        let version = OriginVersion {
            version: change.new_version.clone(),
            changed_by: change.changed_by.clone(),
            changed_at: Some(change.changed_at),
        };
        let announce = |removed: bool, version: OriginVersion| {
            let _ = self.origin_tx.send(OriginChanged {
                workspace_id: link.workspace_id,
                source_link_id: link.id,
                link_kind: link.kind,
                origin_path: change.origin_path.clone(),
                version,
                removed,
            });
        };
        if change.removed {
            // The item is kept as it is; the change is reported on the broadcast only.
            announce(
                true,
                OriginVersion {
                    version: String::new(),
                    changed_by: change.changed_by.clone(),
                    changed_at: Some(change.changed_at),
                },
            );
            return Ok(true);
        }
        let Some(item) = existing else {
            // A new file at the origin: read like any other file of the link.
            self.read_new_origin_file(link, &change).await;
            announce(false, version);
            return Ok(true);
        };
        match item.state {
            WorkingCopyState::Read | WorkingCopyState::Pushed => {
                let fetched = self.refetch(link, &item.origin_path).await?;
                let place = link
                    .place_id
                    .ok_or(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"))?;
                store::check_item(&item.origin_path, &fetched.version)?;
                self.files
                    .store_fetched(
                        place,
                        &item.origin_path,
                        fetched.bytes,
                        fetched.changed_by,
                        fetched.changed_at,
                    )
                    .await
                    .map_err(file_failed)?;
                sqlx::query(
                    "UPDATE working_copy_items SET origin_version_read = ?2, state = 'read', read_at = ?3, \
                     origin_changed_by = ?4, origin_changed_at = ?5 WHERE id = ?1",
                )
                .bind(item.id.to_string())
                .bind(&fetched.version)
                .bind(Timestamp::now().to_wire())
                .bind(change.changed_by.as_deref())
                .bind(change.changed_at.to_wire())
                .execute(&self.pool)
                .await?;
            }
            WorkingCopyState::AwaitingPush => {
                sqlx::query(
                    "UPDATE working_copy_items SET state = 'origin_changed', origin_changed_by = ?2, origin_changed_at = ?3 WHERE id = ?1",
                )
                .bind(item.id.to_string())
                .bind(change.changed_by.as_deref())
                .bind(change.changed_at.to_wire())
                .execute(&self.pool)
                .await?;
            }
            WorkingCopyState::Changed | WorkingCopyState::OriginChanged => {
                sqlx::query("UPDATE working_copy_items SET origin_changed_by = ?2, origin_changed_at = ?3 WHERE id = ?1")
                    .bind(item.id.to_string())
                    .bind(change.changed_by.as_deref())
                    .bind(change.changed_at.to_wire())
                    .execute(&self.pool)
                    .await?;
            }
        }
        announce(false, version);
        Ok(true)
    }

    /// Fetches the current bytes of the item at `origin_path`, through the adapter that tracks it.
    async fn refetch(
        &self,
        link: &SourceLink,
        origin_path: &str,
    ) -> Result<crate::remote::Fetched, WorkspaceError> {
        let reference = match link.kind {
            SourceLinkKind::SharepointLocation => {
                let location = self
                    .sharepoint
                    .resolve_origin(&link.origin_location)
                    .await
                    .map_err(|e| map_adapter(&e))?;
                let found = self
                    .sharepoint
                    .item_at(&location, origin_path)
                    .await
                    .map_err(|e| map_adapter(&e))?
                    .ok_or(WorkspaceError::SourceNotFound)?;
                ItemRef::Sharepoint(found.item)
            }
            SourceLinkKind::ConfluenceLocation => {
                let confluence = self
                    .confluence
                    .as_ref()
                    .ok_or(WorkspaceError::ConfluenceUnavailable)?;
                ItemRef::Confluence(
                    confluence
                        .item_for_path(link.id, origin_path)
                        .await
                        .map_err(|e| map_adapter(&e))?,
                )
            }
            _ => return Err(WorkspaceError::Invalid("SRC_KIND_MISMATCH")),
        };
        self.fetch_item(link, &reference).await
    }

    async fn read_new_origin_file(&self, link: &SourceLink, change: &Change) {
        let Some(place) = link.place_id else { return };
        let outcome: Result<(), WorkspaceError> = async {
            let fetched = self.refetch(link, &change.origin_path).await?;
            store::check_item(&change.origin_path, &fetched.version)?;
            self.files
                .store_fetched(
                    place,
                    &change.origin_path,
                    fetched.bytes,
                    fetched.changed_by,
                    fetched.changed_at,
                )
                .await
                .map_err(file_failed)?;
            match store::insert_item(
                &self.pool,
                link.id,
                &change.origin_path,
                &fetched.version,
                1,
                WorkingCopyState::Read,
                None,
            )
            .await
            {
                Ok(_) | Err(WorkspaceError::Invalid("WCI_DUPLICATE")) => Ok(()),
                Err(error) => Err(error),
            }
        }
        .await;
        if let Err(error) = outcome {
            log::link_item_read_failed(link.id, error.code());
        }
    }
}
