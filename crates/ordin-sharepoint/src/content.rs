//! Fetching, version reads, existence checks and path lookups (Internal Logic 3, 5.1 and 8.2).

use crate::adapter::{valid_name, SharePointAdapter, MAX_FILE_BYTES};
use crate::error::SharePointError;
use crate::graph::Req;
use crate::types::{
    FetchedItem, SharePointItem, SharePointItemRef, SharePointLocation, SharePointVersion,
};
use crate::wire::{DriveItem, ITEM_SELECT};

impl SharePointAdapter {
    /// Internal Logic 3.1: the bytes and the version that was read. A file that changes during the
    /// download is read again once, then refused with `SP_ORIGIN_CHANGED`.
    pub async fn fetch(&self, item: &SharePointItemRef) -> Result<FetchedItem, SharePointError> {
        let mut attempt = 0;
        loop {
            let first = self.item_meta(&item.drive_id, Some(&item.item_id)).await?;
            if first.is_folder() {
                return Err(SharePointError::LocationInvalid);
            }
            if first.size.unwrap_or(0) > MAX_FILE_BYTES {
                return Err(SharePointError::TooLarge);
            }
            let content = self
                .graph
                .call(
                    Req::get(
                        "fetch_content",
                        self.graph.url(
                            &["drives", &item.drive_id, "items", &item.item_id, "content"],
                            &[],
                        ),
                    )
                    .transfer(),
                )
                .await?;
            let second = self.item_meta(&item.drive_id, Some(&item.item_id)).await?;
            if second.ctag == first.ctag {
                let label = self.latest_label(&item.drive_id, &item.item_id).await?;
                return Ok(FetchedItem {
                    item: item.clone(),
                    bytes: content.body.to_vec(),
                    version: second.version(label)?,
                });
            }
            if attempt >= 1 {
                return Err(SharePointError::OriginChanged {
                    current: second.version(None)?,
                });
            }
            attempt += 1;
        }
    }

    /// Internal Logic 3.2: metadata and the latest version label only.
    pub async fn item_version(
        &self,
        item: &SharePointItemRef,
    ) -> Result<SharePointVersion, SharePointError> {
        Ok(self.version_of(item).await?.1)
    }

    /// Internal Logic 5.1: `name` 1–255 characters without SharePoint's forbidden ones; 200 is true, 404 false.
    pub async fn exists(
        &self,
        folder: &SharePointLocation,
        name: &str,
    ) -> Result<bool, SharePointError> {
        if !valid_name(name) {
            return Err(SharePointError::LocationInvalid);
        }
        let reply = self.lookup(folder, &[name]).await?;
        Ok(reply.is_some())
    }

    /// Internal Logic 8.2: the item at `path` (one or more `/`-separated segments) under `folder`.
    pub async fn item_at(
        &self,
        folder: &SharePointLocation,
        path: &str,
    ) -> Result<Option<SharePointItem>, SharePointError> {
        let segments: Vec<&str> = path.split('/').collect();
        if segments.iter().any(|s| !valid_name(s)) {
            return Err(SharePointError::LocationInvalid);
        }
        let Some(found) = self.lookup(folder, &segments).await? else {
            return Ok(None);
        };
        let is_folder = found.is_folder();
        let reference = SharePointItemRef {
            drive_id: folder.drive_id.clone(),
            item_id: found.id.clone(),
        };
        let version = if is_folder {
            None
        } else {
            let label = self
                .latest_label(&reference.drive_id, &reference.item_id)
                .await?;
            Some(found.version(label)?)
        };
        Ok(Some(SharePointItem {
            item: reference,
            name: found.name.clone(),
            path: if is_folder {
                format!("{path}/")
            } else {
                path.to_owned()
            },
            is_folder,
            size_bytes: found.size.unwrap_or(0),
            version,
        }))
    }

    /// `GET …/{parent-id}:/{path}`; 404 is `None`.
    async fn lookup(
        &self,
        folder: &SharePointLocation,
        segments: &[&str],
    ) -> Result<Option<DriveItem>, SharePointError> {
        if !folder.is_folder {
            return Err(SharePointError::LocationInvalid);
        }
        let mut parts = Self::parent_segments(folder);
        parts.extend(segments.iter().map(|s| (*s).to_owned()));
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let reply = self
            .graph
            .call(
                Req::get(
                    "lookup_item",
                    self.graph.url(&parts, &[("$select", ITEM_SELECT)]),
                )
                .pass(&[404]),
            )
            .await?;
        if reply.status == 404 {
            return Ok(None);
        }
        Ok(Some(reply.json()?))
    }
}
