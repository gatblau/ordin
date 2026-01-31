//! Freshness (*Internal Logic* C, with O for places of another computer and the ADR-106 Jira
//! rule): the two change listeners, the lagged resync and `check_page`.

use std::sync::Arc;

use ordin_files::{FileError, PlaceChange, PlaceChangeKind};
use ordin_types::domain::places::{KnowledgePageState, SourceLinkKind};
use ordin_types::time::Timestamp;
use ordin_types::{KnowledgePageId, PlaceId};
use ordin_workspaces::OriginChanged;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::error::KnowledgeError;
use crate::names::place_location;
use crate::service::{lock, KnowledgeService, CHECK_BUDGET};
use crate::store::{self, PageRow, StoredSource};

/// What one source's check found.
enum Found {
    Version {
        version: String,
        changed_by: Option<String>,
        changed_at: Option<Timestamp>,
    },
    /// The source no longer exists (`NotFound` / `None`): marked "removed" (*chosen here*).
    Gone,
    /// A place that is not on this computer (O): neither marked nor counted as unchecked.
    OtherComputer,
    Unchecked,
}

const CITING: &str = "SELECT rowid, id, workspace_id, title, body_markdown, written_by, edited_by_person, \
    sources, state, last_changed_at, place_id, relative_path, file_version, state_before_removal, bin_ticket, \
    memory_indexed_version, memory_index_attempts, memory_next_attempt_at, memory_removed, created_at, \
    shared_by, shared_at, first_exported_at, extra_header, tags, aliases, title_refused, links_built_version, \
    links_truncated, page_type, description, okf_tier, okf_status, status_text, stale_after, verified, generated \
    FROM knowledge_pages p WHERE p.state <> 'removed' AND (?3 IS NULL OR p.workspace_id = ?3) \
    AND EXISTS (SELECT 1 FROM json_each(p.sources) s WHERE json_extract(s.value, '$.origin') = ?1 \
                AND json_extract(s.value, '$.location') = ?2)";

impl KnowledgeService {
    pub(crate) fn note_place(&self, place: PlaceId, exists: bool) {
        let mut missing = lock(&self.missing_places);
        if exists {
            missing.remove(&place);
        } else {
            missing.insert(place);
        }
    }

    /// SA-KTEAM-03: at least one `place` source names a place this installation does not have.
    pub(crate) fn cites_other_computer(&self, sources: &[StoredSource]) -> bool {
        let missing = lock(&self.missing_places);
        sources.iter().any(|s| {
            s.origin == "place"
                && place_location(&s.location).is_some_and(|(p, _)| missing.contains(&p))
        })
    }

    async fn citing_pages(
        &self,
        origin: &str,
        location: &str,
        workspace: Option<ordin_types::WorkspaceId>,
    ) -> Result<Vec<PageRow>, KnowledgeError> {
        let pool = self.pool()?;
        let rows = sqlx::query(CITING)
            .bind(origin)
            .bind(location)
            .bind(workspace.map(|w| w.to_string()))
            .fetch_all(pool)
            .await?;
        rows.iter().map(store::page_from).collect()
    }

    /// Marks one source of a page and sets the page `may_be_out_of_date` when it was `current`.
    async fn mark_source(
        &self,
        row: &PageRow,
        source: &StoredSource,
        newer: &str,
        changed_by: Option<&str>,
        changed_at: Option<Timestamp>,
    ) -> Result<bool, KnowledgeError> {
        if source.version_read == newer {
            return Ok(false);
        }
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let open = store::upsert_mark(
            &mut tx,
            row.id,
            &source.origin,
            &source.location,
            newer,
            changed_by,
            changed_at,
        )
        .await?;
        let marked = open && row.state == KnowledgePageState::Current;
        if open {
            store::set_state(&mut tx, row.id, KnowledgePageState::MayBeOutOfDate).await?;
        }
        let open_marks = store::open_marks(&mut tx, row.id).await?;
        tx.commit().await?;
        if open {
            tracing::info!(component = "knowledge", page_id = %row.id, open_marks, "knowledge_page_marked");
            self.journal(
                "page_marked",
                Some(row.workspace_id),
                None,
                "A knowledge page may be out of date.",
                json!({ "page_id": row.id }),
            )
            .await;
        }
        Ok(marked)
    }

    pub(crate) async fn file_listener(self: Arc<Self>, shutdown: CancellationToken) {
        let mut rx = self.files.subscribe_changes();
        loop {
            let next = tokio::select! {
                _ = shutdown.cancelled() => return,
                n = rx.recv() => n,
            };
            match next {
                Ok(change) => {
                    if let Err(e) = self.on_place_change(change).await {
                        tracing::warn!(
                            component = "knowledge",
                            code = e.code(),
                            "knowledge_change_failed"
                        );
                    }
                }
                Err(RecvError::Lagged(_)) => self.resync().await,
                Err(RecvError::Closed) => {
                    shutdown.cancelled().await;
                    return;
                }
            }
        }
    }

    pub(crate) async fn origin_listener(self: Arc<Self>, shutdown: CancellationToken) {
        let mut rx = self.workspaces.subscribe_origin_changes();
        loop {
            let next = tokio::select! {
                _ = shutdown.cancelled() => return,
                n = rx.recv() => n,
            };
            match next {
                Ok(change) => {
                    if let Err(e) = self.on_origin_change(change).await {
                        tracing::warn!(
                            component = "knowledge",
                            code = e.code(),
                            "knowledge_change_failed"
                        );
                    }
                }
                Err(RecvError::Lagged(_)) => self.resync().await,
                Err(RecvError::Closed) => {
                    shutdown.cancelled().await;
                    return;
                }
            }
        }
    }

    /// C-3: every non-removed page, sequentially, each through C-4 with its own 5 s budget.
    pub(crate) async fn resync(&self) {
        let pages = match self.conn().await {
            Ok(mut conn) => store::all_live_pages(&mut conn).await.unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        tracing::info!(
            component = "knowledge",
            pages = pages.len(),
            "knowledge_freshness_resync"
        );
        for p in pages {
            let _ = self.check_page(p.id).await;
        }
    }

    /// C-1: one change of a place (page files of `knowledge/` per the ADR-104 rules, then the
    /// marking of pages citing the file).
    pub(crate) async fn on_place_change(&self, change: PlaceChange) -> Result<(), KnowledgeError> {
        self.on_own_folder_change(&change).await?;
        let (paths, newer): (Vec<&str>, Option<String>) = match &change.change {
            PlaceChangeKind::Created | PlaceChangeKind::Modified => (
                vec![change.path.as_str()],
                change.version.map(|v| v.to_string()),
            ),
            PlaceChangeKind::Removed => (vec![change.path.as_str()], Some("removed".to_owned())),
            PlaceChangeKind::Renamed { from } => (vec![from.as_str()], Some("removed".to_owned())),
        };
        for path in paths {
            let location = format!("place:{}:{path}", change.place_id);
            let citing = self.citing_pages("place", &location, None).await?;
            for row in citing {
                match &newer {
                    Some(v) => {
                        for s in row
                            .sources
                            .iter()
                            .filter(|s| s.origin == "place" && s.location == location)
                        {
                            self.mark_source(&row, s, v, None, Some(change.observed_at))
                                .await?;
                        }
                    }
                    None => {
                        let _ = self.check_page(row.id).await;
                    }
                }
            }
        }
        Ok(())
    }

    /// C-2 (ADR-106: Jira, local folders, repository folders and remotes mark nothing).
    pub(crate) async fn on_origin_change(
        &self,
        change: OriginChanged,
    ) -> Result<(), KnowledgeError> {
        let origin = match change.link_kind {
            SourceLinkKind::SharepointLocation => "sharepoint",
            SourceLinkKind::ConfluenceLocation => "confluence",
            SourceLinkKind::LocalFolder
            | SourceLinkKind::GitRepositoryFolder
            | SourceLinkKind::GitRemote
            | SourceLinkKind::JiraIssue => return Ok(()),
        };
        let newer = if change.removed {
            "removed".to_owned()
        } else {
            change.version.version.clone()
        };
        let citing = self
            .citing_pages(origin, &change.origin_path, Some(change.workspace_id))
            .await?;
        for row in citing {
            for s in row
                .sources
                .iter()
                .filter(|s| s.origin == origin && s.location == change.origin_path)
            {
                self.mark_source(
                    &row,
                    s,
                    &newer,
                    change.version.changed_by.as_deref(),
                    change.version.changed_at,
                )
                .await?;
                if let Err(e) = self.memory.source_changed(&s.location, &newer).await {
                    tracing::warn!(
                        component = "knowledge",
                        code = e.code(),
                        "knowledge_memory_source_failed"
                    );
                }
            }
        }
        Ok(())
    }

    async fn find(&self, row: &PageRow, s: &StoredSource) -> Found {
        match s.origin.as_str() {
            "place" => {
                let Some((place, path)) = place_location(&s.location) else {
                    return Found::Unchecked;
                };
                match self.files.current_version(place, &path).await {
                    Ok(Some(v)) => {
                        self.note_place(place, true);
                        Found::Version {
                            version: v.version.to_string(),
                            changed_by: None,
                            changed_at: Some(v.created_at),
                        }
                    }
                    Ok(None) | Err(FileError::NotFound) => {
                        self.note_place(place, true);
                        Found::Gone
                    }
                    Err(FileError::PlaceNotFound) => {
                        self.note_place(place, false);
                        Found::OtherComputer
                    }
                    Err(_) => Found::Unchecked,
                }
            }
            "sharepoint" | "confluence" => {
                let kind = if s.origin == "sharepoint" {
                    SourceLinkKind::SharepointLocation
                } else {
                    SourceLinkKind::ConfluenceLocation
                };
                match self
                    .workspaces
                    .origin_version(row.workspace_id, kind, &s.location)
                    .await
                {
                    Ok(Some(v)) => Found::Version {
                        version: v.version,
                        changed_by: v.changed_by,
                        changed_at: v.changed_at,
                    },
                    Ok(None) | Err(ordin_workspaces::WorkspaceError::NotFound) => Found::Gone,
                    Err(_) => Found::Unchecked,
                }
            }
            // SA-OKF-05: freshness marking skips `okf` sources.
            _ => Found::OtherComputer,
        }
    }

    /// C-4: re-checks every source now, concurrently and within 5 s in total.
    pub async fn check_page(
        &self,
        page_id: KnowledgePageId,
    ) -> Result<KnowledgePageState, KnowledgeError> {
        self.desktop_only()?;
        let mut conn = self.conn().await?;
        let Some(row) = store::page(&mut conn, page_id).await? else {
            drop(conn);
            // SA-KREAD-04: a read page is always current and reads no source.
            let read = crate::read::is_read_page_id(&mut *self.conn().await?, page_id).await?;
            return if read {
                Ok(KnowledgePageState::Current)
            } else {
                Err(KnowledgeError::PageNotFound)
            };
        };
        drop(conn);
        if row.state == KnowledgePageState::Removed || row.sources.is_empty() {
            return Ok(row.state);
        }
        let deadline = tokio::time::Instant::now() + CHECK_BUDGET;
        let row_ref = &row;
        let checks = row.sources.iter().map(|s| async move {
            match tokio::time::timeout_at(deadline, self.find(row_ref, s)).await {
                Ok(found) => (s, found),
                Err(_) => (s, Found::Unchecked),
            }
        });
        let results = futures::future::join_all(checks).await;
        let mut unchecked = 0u32;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let mut newly_open = false;
        for (s, found) in &results {
            let (newer, by, at) = match found {
                Found::Version {
                    version,
                    changed_by,
                    changed_at,
                } => (version.clone(), changed_by.clone(), *changed_at),
                Found::Gone => ("removed".to_owned(), None, None),
                Found::OtherComputer => continue,
                Found::Unchecked => {
                    unchecked += 1;
                    continue;
                }
            };
            if newer == s.version_read {
                store::delete_mark(&mut tx, row.id, &s.origin, &s.location).await?;
            } else {
                let open = store::upsert_mark(
                    &mut tx,
                    row.id,
                    &s.origin,
                    &s.location,
                    &newer,
                    by.as_deref(),
                    at,
                )
                .await?;
                newly_open |= open;
            }
        }
        let open = store::open_marks(&mut tx, row.id).await?;
        let state = if unchecked > 0 {
            tracing::info!(component = "knowledge", page_id = %row.id, unchecked, "knowledge_check_incomplete");
            row.state
        } else if open > 0 {
            KnowledgePageState::MayBeOutOfDate
        } else {
            KnowledgePageState::Current
        };
        store::set_state(&mut tx, row.id, state).await?;
        tx.commit().await?;
        if newly_open
            && state == KnowledgePageState::MayBeOutOfDate
            && row.state == KnowledgePageState::Current
        {
            tracing::info!(component = "knowledge", page_id = %row.id, open_marks = open, "knowledge_page_marked");
            self.journal(
                "page_marked",
                Some(row.workspace_id),
                None,
                "A knowledge page may be out of date.",
                json!({ "page_id": row.id }),
            )
            .await;
        }
        Ok(state)
    }
}
