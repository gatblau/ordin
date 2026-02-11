//! Reading a remote link into its working copy (WorkspaceService.md Internal Logic 4): the walk,
//! the fetches, the unread list and `read_item`.

use std::collections::VecDeque;
use std::time::Instant;

use futures::StreamExt;
use ordin_confluence::{ConfluenceLocation, RemoteItemRef};
use ordin_sharepoint::{SharePointItemRef, SharePointLocation};
use ordin_types::domain::places::{
    SourceLink, SourceLinkKind, SourceLinkState, WorkingCopyItem, WorkingCopyState,
};
use ordin_types::time::Timestamp;
use ordin_types::{CodedError, PlaceId, SourceLinkId};
use sqlx::Row;

use crate::error::{file_failed, WorkspaceError};
use crate::links::map_adapter;
use crate::log;
use crate::service::{lock, WorkspaceService};
use crate::store;
use crate::types::LinkProgress;
use crate::{FETCH_CONCURRENCY, LINK_MAX_FOLDER_LEVELS, LINK_MAX_ITEMS, LINK_READ_BUDGET};

/// A listed file of a remote link and the reference its adapter fetches it by.
pub(crate) struct Listed {
    pub origin_path: String,
    pub size: u64,
    pub item: ItemRef,
}

#[derive(Debug, Clone)]
pub(crate) enum ItemRef {
    Sharepoint(SharePointItemRef),
    Confluence(RemoteItemRef),
    JiraAttachment(String),
}

impl ItemRef {
    /// The JSON stored in `working_copy_unread.item_ref`: the adapter's own reference, or
    /// `{"attachment_id": …}` for Jira.
    pub(crate) fn to_json(&self) -> String {
        match self {
            ItemRef::Sharepoint(item) => serde_json::to_string(item),
            ItemRef::Confluence(item) => serde_json::to_string(item),
            ItemRef::JiraAttachment(id) => {
                serde_json::to_string(&serde_json::json!({ "attachment_id": id }))
            }
        }
        .unwrap_or_default()
    }

    pub(crate) fn from_json(kind: SourceLinkKind, text: &str) -> Result<ItemRef, WorkspaceError> {
        let damaged = |_| WorkspaceError::DatastoreUnavailable;
        match kind {
            SourceLinkKind::SharepointLocation => serde_json::from_str(text)
                .map(ItemRef::Sharepoint)
                .map_err(damaged),
            SourceLinkKind::ConfluenceLocation => serde_json::from_str(text)
                .map(ItemRef::Confluence)
                .map_err(damaged),
            SourceLinkKind::JiraIssue => {
                let value: serde_json::Value = serde_json::from_str(text).map_err(damaged)?;
                value
                    .get("attachment_id")
                    .and_then(|v| v.as_str())
                    .map(|id| ItemRef::JiraAttachment(id.to_owned()))
                    .ok_or(WorkspaceError::DatastoreUnavailable)
            }
            _ => Err(WorkspaceError::DatastoreUnavailable),
        }
    }
}

/// One fetched item, whichever adapter it came from.
pub(crate) struct Fetched {
    pub bytes: Vec<u8>,
    pub version: String,
    pub changed_by: Option<String>,
    pub changed_at: Timestamp,
}

impl WorkspaceService {
    pub(crate) fn set_progress(&self, link: SourceLinkId, update: impl FnOnce(&mut LinkProgress)) {
        let mut all = lock(&self.progress);
        let entry = all.entry(link).or_insert(LinkProgress {
            source_link_id: link,
            listed: 0,
            read: 0,
            not_read: 0,
            done: false,
        });
        update(entry);
    }

    /// Internal Logic 4.1: the files of the link's location, breadth first, up to `LINK_MAX_ITEMS`
    /// files and `LINK_MAX_FOLDER_LEVELS` levels. A failure after the first file ends the walk with
    /// what was listed; a failure before it is the error.
    pub(crate) async fn walk(&self, link: &SourceLink) -> Result<Vec<Listed>, WorkspaceError> {
        match link.kind {
            SourceLinkKind::SharepointLocation => self.walk_sharepoint(link).await,
            SourceLinkKind::ConfluenceLocation => self.walk_confluence(link).await,
            _ => Ok(Vec::new()),
        }
    }

    async fn walk_sharepoint(&self, link: &SourceLink) -> Result<Vec<Listed>, WorkspaceError> {
        let root = self
            .sharepoint
            .resolve_origin(&link.origin_location)
            .await
            .map_err(|e| map_adapter(&e))?;
        let mut out: Vec<Listed> = Vec::new();
        let mut queue: VecDeque<(SharePointLocation, String, usize)> = VecDeque::new();
        queue.push_back((root, String::new(), 0));
        while let Some((location, prefix, depth)) = queue.pop_front() {
            let mut cursor = None;
            loop {
                let page = match self.sharepoint.list_children(&location, cursor).await {
                    Ok(page) => page,
                    Err(error) if out.is_empty() => return Err(map_adapter(&error)),
                    Err(error) => {
                        log::link_item_read_failed(link.id, error.code());
                        return Ok(out);
                    }
                };
                for item in page.items {
                    if item.is_folder {
                        if depth + 1 < LINK_MAX_FOLDER_LEVELS {
                            let child = SharePointLocation {
                                site_id: location.site_id.clone(),
                                site_name: location.site_name.clone(),
                                drive_id: location.drive_id.clone(),
                                drive_name: location.drive_name.clone(),
                                item_id: Some(item.item.item_id.clone()),
                                path: if location.path.is_empty() {
                                    item.name.clone()
                                } else {
                                    format!("{}/{}", location.path, item.name)
                                },
                                is_folder: true,
                                web_url: String::new(),
                            };
                            queue.push_back((child, format!("{prefix}{}/", item.name), depth + 1));
                        }
                        continue;
                    }
                    if out.len() >= LINK_MAX_ITEMS {
                        return Ok(out);
                    }
                    out.push(Listed {
                        origin_path: format!("{prefix}{}", item.name),
                        size: item.size_bytes,
                        item: ItemRef::Sharepoint(item.item),
                    });
                }
                self.set_progress(link.id, |p| p.listed = out.len() as u64);
                match page.next_cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
        }
        Ok(out)
    }

    async fn walk_confluence(&self, link: &SourceLink) -> Result<Vec<Listed>, WorkspaceError> {
        let confluence = self
            .confluence
            .as_ref()
            .ok_or(WorkspaceError::ConfluenceUnavailable)?;
        let root = confluence
            .resolve_origin(&link.origin_location)
            .await
            .map_err(|e| map_adapter(&e))?;
        let mut out: Vec<Listed> = Vec::new();
        // WS-RD-1: a link to one page reads that page first, then its children and attachments.
        // spec: ordin-desktop/components/WorkspaceService#WS-RD-1
        if let Some(page) = confluence
            .linked_page(&root)
            .await
            .map_err(|e| map_adapter(&e))?
        {
            out.push(Listed {
                origin_path: page.path.clone(),
                size: page.size_bytes.unwrap_or(0),
                item: ItemRef::Confluence(page.item),
            });
            self.set_progress(link.id, |p| p.listed = out.len() as u64);
        }
        let mut queue: VecDeque<(ConfluenceLocation, usize)> = VecDeque::new();
        queue.push_back((root, 0));
        while let Some((location, depth)) = queue.pop_front() {
            let mut cursor = None;
            loop {
                let page = match confluence.list_children(&location, cursor).await {
                    Ok(page) => page,
                    Err(error) if out.is_empty() => return Err(map_adapter(&error)),
                    Err(error) => {
                        log::link_item_read_failed(link.id, error.code());
                        return Ok(out);
                    }
                };
                for item in page.items {
                    if let RemoteItemRef::Page { page_id } = &item.item {
                        if item.has_children && depth + 1 < LINK_MAX_FOLDER_LEVELS {
                            queue.push_back((
                                ConfluenceLocation {
                                    space_key: location.space_key.clone(),
                                    space_name: location.space_name.clone(),
                                    page_id: Some(page_id.clone()),
                                    title: item.title.clone(),
                                    web_url: String::new(),
                                },
                                depth + 1,
                            ));
                        }
                    }
                    if out.len() >= LINK_MAX_ITEMS {
                        return Ok(out);
                    }
                    out.push(Listed {
                        origin_path: item.path.clone(),
                        size: item.size_bytes.unwrap_or(0),
                        item: ItemRef::Confluence(item.item),
                    });
                }
                self.set_progress(link.id, |p| p.listed = out.len() as u64);
                match page.next_cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
        }
        Ok(out)
    }

    /// The bytes and version of one item, from its adapter (Jira attachments: `jira.rs`).
    pub(crate) async fn fetch_item(
        &self,
        link: &SourceLink,
        item: &ItemRef,
    ) -> Result<Fetched, WorkspaceError> {
        match item {
            ItemRef::Sharepoint(item) => {
                let fetched = self
                    .sharepoint
                    .fetch(item)
                    .await
                    .map_err(|e| map_adapter(&e))?;
                Ok(Fetched {
                    bytes: fetched.bytes,
                    version: fetched.version.tag,
                    changed_by: fetched.version.changed_by,
                    changed_at: fetched.version.changed_at,
                })
            }
            ItemRef::Confluence(item) => {
                let confluence = self
                    .confluence
                    .as_ref()
                    .ok_or(WorkspaceError::ConfluenceUnavailable)?;
                let fetched = confluence.fetch(item).await.map_err(|e| map_adapter(&e))?;
                Ok(Fetched {
                    bytes: fetched.bytes,
                    version: fetched.version.tag,
                    changed_by: fetched.version.changed_by,
                    changed_at: fetched.version.changed_at,
                })
            }
            ItemRef::JiraAttachment(id) => self.jira_fetch_attachment(link, id).await,
        }
    }

    /// Fetches, writes into the working copy and records the item (Internal Logic 4.2). An item that
    /// already has a row is left to Internal Logic 6.
    async fn read_one(
        &self,
        link: &SourceLink,
        place: PlaceId,
        listed: &Listed,
    ) -> Result<u64, WorkspaceError> {
        store::check_item(&listed.origin_path, "-")?;
        let fetched = self.fetch_item(link, &listed.item).await?;
        let length = fetched.bytes.len() as u64;
        store::check_item(&listed.origin_path, &fetched.version)?;
        self.files
            .store_fetched(
                place,
                &listed.origin_path,
                fetched.bytes,
                fetched.changed_by,
                fetched.changed_at,
            )
            .await
            .map_err(file_failed)?;
        match store::insert_item(
            &self.pool,
            link.id,
            &listed.origin_path,
            &fetched.version,
            1,
            WorkingCopyState::Read,
            None,
        )
        .await
        {
            Ok(_) => {
                store::set_item_size(&self.pool, link.id, &listed.origin_path, length).await;
                Ok(length)
            }
            Err(WorkspaceError::Invalid("WCI_DUPLICATE")) => Ok(length),
            Err(error) => Err(error),
        }
    }

    async fn read_pair(
        &self,
        link: &SourceLink,
        place: PlaceId,
        item: Listed,
    ) -> (Listed, Result<u64, WorkspaceError>) {
        let outcome = self.read_one(link, place, &item).await;
        (item, outcome)
    }

    pub(crate) async fn keep_unread(&self, link: SourceLinkId, listed: &Listed) {
        let _ = sqlx::query(
            "INSERT OR REPLACE INTO working_copy_unread (source_link_id, origin_path, item_ref, size_bytes) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(link.to_string())
        .bind(&listed.origin_path)
        .bind(listed.item.to_json())
        .bind(i64::try_from(listed.size).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await;
    }

    /// The background read of a SharePoint or Confluence link, once at confirmation (Internal Logic 4).
    pub(crate) async fn read_remote_link(&self, link: SourceLink) {
        let started = Instant::now();
        let Some(place) = link.place_id else { return };
        self.set_progress(link.id, |p| {
            *p = LinkProgress {
                source_link_id: link.id,
                listed: 0,
                read: 0,
                not_read: 0,
                done: false,
            }
        });
        let listed = match self.walk(&link).await {
            Ok(listed) => listed,
            Err(error) => {
                log::background_failed("link_walk", error.code());
                self.set_progress(link.id, |p| p.done = true);
                return;
            }
        };
        let listed_count = listed.len() as u64;
        self.set_progress(link.id, |p| p.listed = listed_count);
        // Reading stops at the first file that would take the link past its budget.
        let mut used: u64 = 0;
        let mut within = true;
        let mut fit: Vec<Listed> = Vec::new();
        for item in listed {
            if within && used.saturating_add(item.size) <= LINK_READ_BUDGET {
                used += item.size;
                fit.push(item);
            } else {
                within = false;
                self.keep_unread(link.id, &item).await;
                self.set_progress(link.id, |p| p.not_read += 1);
            }
        }
        let mut results = futures::stream::iter(fit)
            .map(|item| self.read_pair(&link, place, item))
            .buffer_unordered(FETCH_CONCURRENCY);
        while let Some((item, outcome)) = results.next().await {
            match outcome {
                Ok(_) => self.set_progress(link.id, |p| p.read += 1),
                Err(error) => {
                    log::link_item_read_failed(link.id, error.code());
                    self.keep_unread(link.id, &item).await;
                    self.set_progress(link.id, |p| p.not_read += 1);
                }
            }
        }
        drop(results);
        let now = Timestamp::now();
        let _ = sqlx::query("UPDATE source_links SET last_read_at = ?2 WHERE id = ?1")
            .bind(link.id.to_string())
            .bind(now.to_wire())
            .execute(&self.pool)
            .await;
        let (read, not_read) = {
            let all = lock(&self.progress);
            all.get(&link.id).map_or((0, 0), |p| (p.read, p.not_read))
        };
        self.record(
            "source_link_read",
            Some(link.workspace_id),
            "A linked location was read into the workspace.",
            serde_json::json!({ "source_link_id": link.id, "read": read, "not_read": not_read }),
        )
        .await;
        log::source_link_read(
            link.id,
            read,
            not_read,
            started.elapsed().as_millis() as u64,
        );
        self.set_progress(link.id, |p| p.done = true);
        self.changed(link.workspace_id).await;
    }

    /// Tries again the items of the link still in `working_copy_unread` that fit the link's read
    /// budget (Internal Logic 4.3). An item that would take the link past `LINK_READ_BUDGET` stays
    /// unread until first use. A failure is logged and the item stays unread for the next check.
    pub(crate) async fn retry_unread(&self, link: &SourceLink) {
        let Ok(rows) = sqlx::query(
            "SELECT origin_path, size_bytes FROM working_copy_unread WHERE source_link_id = ?1 ORDER BY origin_path",
        )
        .bind(link.id.to_string())
        .fetch_all(&self.pool)
        .await
        else {
            return;
        };
        let mut used = store::read_bytes(&self.pool, link.id).await;
        for row in rows {
            let Ok(origin_path) = row.try_get::<String, _>("origin_path") else {
                continue;
            };
            let size = u64::try_from(row.try_get::<i64, _>("size_bytes").unwrap_or(i64::MAX))
                .unwrap_or(u64::MAX);
            if used.saturating_add(size) > LINK_READ_BUDGET {
                continue;
            }
            match self.read_item(link.id, &origin_path).await {
                Ok(_) => used = used.saturating_add(size),
                Err(error) => log::link_item_read_failed(link.id, error.code()),
            }
        }
    }

    /// Reads one item that was listed but not read (budget or a failure), on first use; for a Jira
    /// item with a newer version at its origin, brings it up to date (§J.4).
    pub async fn read_item(
        &self,
        link: SourceLinkId,
        origin_path: &str,
    ) -> Result<WorkingCopyItem, WorkspaceError> {
        let held = store::load_link(&self.pool, link).await?;
        if held.state != SourceLinkState::Active {
            return Err(WorkspaceError::LinkStateInvalid);
        }
        if let Some(item) = store::item_at(&self.pool, link, origin_path).await? {
            if held.kind == SourceLinkKind::JiraIssue && item.origin_newer_at.is_some() {
                return self.jira_bring_up_to_date(&held, &item).await;
            }
            return Ok(item);
        }
        let place = held
            .place_id
            .ok_or(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"))?;
        let row = sqlx::query("SELECT item_ref FROM working_copy_unread WHERE source_link_id = ?1 AND origin_path = ?2")
            .bind(link.to_string())
            .bind(origin_path)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(WorkspaceError::WorkingCopyItemNotFound)?;
        let item_ref = ItemRef::from_json(held.kind, &row.try_get::<String, _>("item_ref")?)?;
        let fetched = self.fetch_item(&held, &item_ref).await?;
        let length = fetched.bytes.len() as u64;
        store::check_item(origin_path, &fetched.version)?;
        self.files
            .store_fetched(
                place,
                origin_path,
                fetched.bytes,
                fetched.changed_by,
                fetched.changed_at,
            )
            .await
            .map_err(file_failed)?;
        let item = match store::insert_item(
            &self.pool,
            link,
            origin_path,
            &fetched.version,
            1,
            WorkingCopyState::Read,
            None,
        )
        .await
        {
            Ok(item) => {
                store::set_item_size(&self.pool, link, origin_path, length).await;
                item
            }
            Err(WorkspaceError::Invalid("WCI_DUPLICATE")) => {
                store::item_at(&self.pool, link, origin_path)
                    .await?
                    .ok_or(WorkspaceError::WorkingCopyItemNotFound)?
            }
            Err(error) => return Err(error),
        };
        sqlx::query(
            "DELETE FROM working_copy_unread WHERE source_link_id = ?1 AND origin_path = ?2",
        )
        .bind(link.to_string())
        .bind(origin_path)
        .execute(&self.pool)
        .await?;
        self.set_progress(link, |p| {
            p.read += 1;
            p.not_read = p.not_read.saturating_sub(1);
        });
        Ok(item)
    }
}
