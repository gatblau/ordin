//! The read methods this module calls on its dependencies, as crate-private traits so that the
//! logic is tested against doubles; `HomeSearch::new` wraps the concrete services the spec names.

use std::sync::Arc;

use ordin_confluence::{ConfluenceAdapter, ConfluenceError, RemoteItemRef};
use ordin_files::{FileError, FileService, PlaceChange, SearchResults};
use ordin_sharepoint::{SharePointAdapter, SharePointError, SharePointItemRef};
use ordin_types::domain::places::{Place, SourceLink, Workspace};
use ordin_types::time::Timestamp;
use ordin_types::{PlaceId, SourceLinkId, WorkspaceId};
use ordin_workspaces::{OriginChanged, WorkspaceError, WorkspaceListFilter, WorkspaceService};
use tokio::sync::broadcast;

/// An origin change as both adapters report it (`OriginChange`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteChange {
    pub source_link_id: SourceLinkId,
    pub origin_path: String,
    pub changed_at: Timestamp,
    pub removed: bool,
}

/// How an adapter's version read ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VersionRead {
    Tag(String),
    NotFound,
    Failed,
}

#[async_trait::async_trait]
pub(crate) trait FilesPort: Send + Sync {
    fn subscribe_changes(&self) -> broadcast::Receiver<PlaceChange>;
    async fn search(
        &self,
        place_ids: &[PlaceId],
        query: &str,
        limit: u32,
    ) -> Result<SearchResults, FileError>;
    /// The file's hash (`FileBytes.hash`).
    async fn file_hash(&self, place_id: PlaceId, path: &str) -> Result<String, FileError>;
}

#[async_trait::async_trait]
pub(crate) trait WorkspacesPort: Send + Sync {
    async fn list(&self, filter: WorkspaceListFilter) -> Result<Vec<Workspace>, WorkspaceError>;
    async fn places(&self, id: WorkspaceId) -> Result<Vec<Place>, WorkspaceError>;
    async fn source_links(&self, id: WorkspaceId) -> Result<Vec<SourceLink>, WorkspaceError>;
    async fn origin_changes_since(
        &self,
        since: Timestamp,
    ) -> Result<Vec<OriginChanged>, WorkspaceError>;
}

/// One of the two document adapters (SharePoint, Confluence).
#[async_trait::async_trait]
pub(crate) trait RemotePort: Send + Sync {
    /// `location` is the part after `graph:` or `confluence:`.
    async fn version(&self, location: &str) -> VersionRead;
    async fn changes_since(
        &self,
        links: &[SourceLinkId],
        since: Timestamp,
    ) -> Result<Vec<RemoteChange>, String>;
}

#[async_trait::async_trait]
impl FilesPort for FileService {
    fn subscribe_changes(&self) -> broadcast::Receiver<PlaceChange> {
        FileService::subscribe_changes(self)
    }

    async fn search(
        &self,
        place_ids: &[PlaceId],
        query: &str,
        limit: u32,
    ) -> Result<SearchResults, FileError> {
        FileService::search(self, place_ids, query, limit).await
    }

    async fn file_hash(&self, place_id: PlaceId, path: &str) -> Result<String, FileError> {
        Ok(FileService::read_file(self, place_id, path).await?.hash.0)
    }
}

#[async_trait::async_trait]
impl WorkspacesPort for WorkspaceService {
    async fn list(&self, filter: WorkspaceListFilter) -> Result<Vec<Workspace>, WorkspaceError> {
        WorkspaceService::list(self, filter).await
    }

    async fn places(&self, id: WorkspaceId) -> Result<Vec<Place>, WorkspaceError> {
        WorkspaceService::places(self, id).await
    }

    async fn source_links(&self, id: WorkspaceId) -> Result<Vec<SourceLink>, WorkspaceError> {
        WorkspaceService::source_links(self, id).await
    }

    async fn origin_changes_since(
        &self,
        since: Timestamp,
    ) -> Result<Vec<OriginChanged>, WorkspaceError> {
        WorkspaceService::origin_changes_since(self, since).await
    }
}

/// `graph:drives/<drive-id>/items/<item-id>` (2C location form).
pub(crate) fn sharepoint_item(location: &str) -> Option<SharePointItemRef> {
    let rest = location.strip_prefix("drives/")?;
    let (drive_id, item_id) = rest.split_once("/items/")?;
    if drive_id.is_empty() || item_id.is_empty() || item_id.contains('/') {
        return None;
    }
    Some(SharePointItemRef {
        drive_id: drive_id.to_owned(),
        item_id: item_id.to_owned(),
    })
}

/// `confluence:page/<page-id>` or `confluence:attachment/<page-id>/<attachment-id>` (2C forms).
pub(crate) fn confluence_item(location: &str) -> Option<RemoteItemRef> {
    if let Some(page_id) = location.strip_prefix("page/") {
        if page_id.is_empty() || page_id.contains('/') {
            return None;
        }
        return Some(RemoteItemRef::Page {
            page_id: page_id.to_owned(),
        });
    }
    let rest = location.strip_prefix("attachment/")?;
    let (page_id, attachment_id) = rest.split_once('/')?;
    if page_id.is_empty() || attachment_id.is_empty() || attachment_id.contains('/') {
        return None;
    }
    Some(RemoteItemRef::Attachment {
        page_id: page_id.to_owned(),
        attachment_id: attachment_id.to_owned(),
    })
}

#[async_trait::async_trait]
impl RemotePort for SharePointAdapter {
    async fn version(&self, location: &str) -> VersionRead {
        let Some(item) = sharepoint_item(location) else {
            return VersionRead::Failed;
        };
        match self.item_version(&item).await {
            Ok(version) => VersionRead::Tag(version.tag),
            Err(SharePointError::NotFound) => VersionRead::NotFound,
            Err(_) => VersionRead::Failed,
        }
    }

    async fn changes_since(
        &self,
        links: &[SourceLinkId],
        since: Timestamp,
    ) -> Result<Vec<RemoteChange>, String> {
        let changes = SharePointAdapter::changes_since(self, links, since)
            .await
            .map_err(|e| e.to_string())?;
        Ok(changes
            .into_iter()
            .map(|c| RemoteChange {
                source_link_id: c.source_link_id,
                origin_path: c.origin_path,
                changed_at: c.changed_at,
                removed: c.removed,
            })
            .collect())
    }
}

#[async_trait::async_trait]
impl RemotePort for ConfluenceAdapter {
    async fn version(&self, location: &str) -> VersionRead {
        let Some(item) = confluence_item(location) else {
            return VersionRead::Failed;
        };
        match self.item_version(&item).await {
            Ok(version) => VersionRead::Tag(version.tag),
            Err(ConfluenceError::NotFound) => VersionRead::NotFound,
            Err(_) => VersionRead::Failed,
        }
    }

    async fn changes_since(
        &self,
        links: &[SourceLinkId],
        since: Timestamp,
    ) -> Result<Vec<RemoteChange>, String> {
        let changes = ConfluenceAdapter::changes_since(self, links, since)
            .await
            .map_err(|e| e.to_string())?;
        Ok(changes
            .into_iter()
            .map(|c| RemoteChange {
                source_link_id: c.source_link_id,
                origin_path: c.origin_path,
                changed_at: c.changed_at,
                removed: c.removed,
            })
            .collect())
    }
}

/// The dependencies as the logic reads them.
#[derive(Clone)]
pub(crate) struct Ports {
    pub reader: Arc<dyn crate::JournalReader>,
    pub knowledge: Arc<dyn ordin_knowledge::KnowledgeReader>,
    pub files: Arc<dyn FilesPort>,
    pub workspaces: Arc<dyn WorkspacesPort>,
    pub sharepoint: Arc<dyn RemotePort>,
    pub confluence: Option<Arc<dyn RemotePort>>,
}
