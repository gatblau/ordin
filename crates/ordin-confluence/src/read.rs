//! Reading (ConfluenceAdapter.md Internal Logic 2): finding locations, listing a tree, fetching a page or
//! an attachment with its version. This adapter never writes to a place.

use ordin_types::{Cursor, Page};

use crate::client::{next_relative, relative, Req};
use crate::error::ConfluenceError;
use crate::push::{MAX_ATTACHMENT_BYTES, MAX_PAGE_BODY_BYTES};
use crate::store;
use crate::types::{ConfluenceLocation, FetchedItem, RemoteItem, RemoteItemRef, RemoteVersion};
use crate::wire::{
    cql_quote, is_page_id, item_key, join, page_path, page_prefix, path_title, Listing, SearchHit,
    WireAttachment, WirePage, WireSpace, WireVersion,
};
use crate::ConfluenceAdapter;

/// Search text is 1–200 characters after trimming.
const MAX_QUERY_CHARS: usize = 200;
/// `confluence.find` returns at most 25 locations.
const MAX_FIND_LIMIT: u32 = 25;
/// A listing's `limit`.
const LIST_LIMIT: &str = "250";

fn page_children(page_id: &str) -> String {
    relative(
        &["wiki", "api", "v2", "pages", page_id, "children"],
        &[("limit", LIST_LIMIT)],
    )
}

fn page_attachments(page_id: &str) -> String {
    relative(
        &["wiki", "api", "v2", "pages", page_id, "attachments"],
        &[("limit", LIST_LIMIT)],
    )
}

pub(crate) fn search_url(cql: &str, limit: &str) -> String {
    relative(
        &["wiki", "rest", "api", "search"],
        &[("cql", cql), ("limit", limit)],
    )
}

fn space_key_valid(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '~' | '-' | '_' | '.'))
}

/// The key at the end of a search row's `displayUrl`: `/spaces/PROC`, or `/display/PROC` on older sites.
fn key_of_display_url(url: &str) -> Option<&str> {
    url.rsplit('/').find(|part| !part.is_empty())
}

impl ConfluenceAdapter {
    fn web_url(&self, tail: Option<&str>) -> Result<String, ConfluenceError> {
        Ok(format!(
            "{}/wiki{}",
            self.transport.site()?,
            tail.unwrap_or_default()
        ))
    }

    /// The page with its body and version.
    pub(crate) async fn read_page(&self, page_id: &str) -> Result<WirePage, ConfluenceError> {
        let url = relative(
            &["wiki", "api", "v2", "pages", page_id],
            &[("body-format", "storage")],
        );
        self.transport
            .call(Req::get("page", url).transfer())
            .await?
            .json()
    }

    pub(crate) async fn space_by_key(&self, key: &str) -> Result<WireSpace, ConfluenceError> {
        let url = relative(&["wiki", "api", "v2", "spaces"], &[("keys", key)]);
        let spaces: Listing<WireSpace> = self
            .transport
            .call(Req::get("space_by_key", url))
            .await?
            .json()?;
        spaces
            .results
            .into_iter()
            .next()
            .ok_or(ConfluenceError::NotFound)
    }

    /// Every attachment of a page, following `_links.next`.
    pub(crate) async fn attachments(
        &self,
        page_id: &str,
    ) -> Result<Vec<WireAttachment>, ConfluenceError> {
        let mut found = Vec::new();
        let mut url = page_attachments(page_id);
        loop {
            let listing: Listing<WireAttachment> = self
                .transport
                .call(Req::get("attachments", url))
                .await?
                .json()?;
            found.extend(listing.results);
            match listing.links.next {
                Some(next) => url = next_relative(&next)?,
                None => return Ok(found),
            }
        }
    }

    /// Every child page of a page, following `_links.next`.
    pub(crate) async fn children(&self, page_id: &str) -> Result<Vec<WirePage>, ConfluenceError> {
        let mut found = Vec::new();
        let mut url = page_children(page_id);
        loop {
            let listing: Listing<WirePage> = self
                .transport
                .call(Req::get("children", url))
                .await?
                .json()?;
            found.extend(listing.results);
            match listing.links.next {
                Some(next) => url = next_relative(&next)?,
                None => return Ok(found),
            }
        }
    }

    /// Every top page of a space, following `_links.next`.
    pub(crate) async fn root_pages(
        &self,
        space_id: &str,
    ) -> Result<Vec<WirePage>, ConfluenceError> {
        let mut found = Vec::new();
        let mut url = Self::root_pages_url(space_id);
        loop {
            let listing: Listing<WirePage> = self
                .transport
                .call(Req::get("root_pages", url))
                .await?
                .json()?;
            found.extend(listing.results);
            match listing.links.next {
                Some(next) => url = next_relative(&next)?,
                None => return Ok(found),
            }
        }
    }

    fn root_pages_url(space_id: &str) -> String {
        relative(
            &["wiki", "api", "v2", "spaces", space_id, "pages"],
            &[("depth", "root"), ("limit", LIST_LIMIT)],
        )
    }

    /// One attachment, from its page's listing: Confluence's table names no read of a single attachment.
    pub(crate) async fn read_attachment(
        &self,
        page_id: &str,
        attachment_id: &str,
    ) -> Result<WireAttachment, ConfluenceError> {
        self.attachments(page_id)
            .await?
            .into_iter()
            .find(|a| a.id == attachment_id)
            .ok_or(ConfluenceError::NotFound)
    }

    /// The current version of an item without naming its author, for the comparisons of a push.
    pub(crate) async fn version_read(
        &self,
        item: &RemoteItemRef,
    ) -> Result<RemoteVersion, ConfluenceError> {
        match item {
            RemoteItemRef::Page { page_id } => self.read_page(page_id).await?.version()?.remote(),
            RemoteItemRef::Attachment {
                page_id,
                attachment_id,
            } => self
                .read_attachment(page_id, attachment_id)
                .await?
                .version()?
                .remote(),
        }
    }

    /// A wire version with its author named.
    pub(crate) async fn named(
        &self,
        version: &WireVersion,
    ) -> Result<RemoteVersion, ConfluenceError> {
        let mut remote = version.remote()?;
        remote.changed_by = self.transport.author(version.account()).await;
        Ok(remote)
    }

    /// Internal Logic 2.1.
    pub async fn find_locations(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<ConfluenceLocation>, ConfluenceError> {
        let query = query.trim();
        if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
            return Err(ConfluenceError::QueryInvalid);
        }
        let limit = limit.clamp(1, MAX_FIND_LIMIT);
        let cql = format!("(type=space OR type=page) AND title ~ {}", cql_quote(query));
        let hits: Listing<SearchHit> = self
            .transport
            .call(Req::get(
                "find_locations",
                search_url(&cql, &limit.to_string()),
            ))
            .await?
            .json()?;
        let mut locations = Vec::with_capacity(hits.results.len());
        for hit in hits.results {
            if let Some(location) = self.location_of_hit(hit)? {
                locations.push(location);
            }
        }
        locations.truncate(limit as usize);
        Ok(locations)
    }

    /// A row that is neither a space nor a page is left out.
    fn location_of_hit(
        &self,
        hit: SearchHit,
    ) -> Result<Option<ConfluenceLocation>, ConfluenceError> {
        let web_url = self.web_url(hit.url.as_deref())?;
        if let Some(space) = hit.space {
            return Ok(Some(ConfluenceLocation {
                space_key: space.key,
                title: space.name.clone(),
                space_name: space.name,
                page_id: None,
                web_url,
            }));
        }
        let (Some(content), Some(container)) = (hit.content, hit.container) else {
            return Ok(None);
        };
        if content.kind != "page" {
            return Ok(None);
        }
        let space_key = key_of_display_url(&container.display_url)
            .ok_or(ConfluenceError::ResponseInvalid)?
            .to_owned();
        Ok(Some(ConfluenceLocation {
            space_key,
            space_name: container.title,
            page_id: Some(content.id),
            title: content.title,
            web_url,
        }))
    }

    /// `confluence:space/{key}` or `confluence:page/{pageId}`.
    pub fn origin_location(loc: &ConfluenceLocation) -> String {
        match &loc.page_id {
            Some(page_id) => format!("confluence:page/{page_id}"),
            None => format!("confluence:space/{}", loc.space_key),
        }
    }

    pub async fn resolve_origin(
        &self,
        origin_location: &str,
    ) -> Result<ConfluenceLocation, ConfluenceError> {
        if let Some(key) = origin_location.strip_prefix("confluence:space/") {
            if !space_key_valid(key) {
                return Err(ConfluenceError::LocationInvalid);
            }
            let space = self.space_by_key(key).await?;
            return Ok(ConfluenceLocation {
                web_url: self.web_url(space.links.webui.as_deref())?,
                space_key: space.key,
                title: space.name.clone(),
                space_name: space.name,
                page_id: None,
            });
        }
        let page_id = origin_location
            .strip_prefix("confluence:page/")
            .filter(|id| is_page_id(id))
            .ok_or(ConfluenceError::LocationInvalid)?;
        // `ConfluenceLocation` needs the space's key and name, and a page answers only
        // `spaceId`, but the surface table has no read of a space by id. The search row answers both, so
        // it is used here with the `id=` clause its change-tracking form already has.
        let hits: Listing<SearchHit> = self
            .transport
            .call(Req::get(
                "resolve_origin",
                search_url(&format!("id={page_id}"), "1"),
            ))
            .await?
            .json()?;
        let hit = hits
            .results
            .into_iter()
            .next()
            .ok_or(ConfluenceError::NotFound)?;
        self.location_of_hit(hit)?.ok_or(ConfluenceError::NotFound)
    }

    /// The tracked path of a page without its suffix, else the title alone (Internal Logic 2.2).
    async fn prefix_of(&self, loc: &ConfluenceLocation) -> Result<String, ConfluenceError> {
        let Some(page_id) = &loc.page_id else {
            return Ok(String::new());
        };
        let key = item_key(&RemoteItemRef::Page {
            page_id: page_id.clone(),
        });
        Ok(match store::known_path(self.pool()?, &key).await? {
            Some(path) => page_prefix(&path),
            None => path_title(&loc.title),
        })
    }

    pub(crate) async fn page_item(
        &self,
        page: &WirePage,
        prefix: &str,
    ) -> Result<RemoteItem, ConfluenceError> {
        // The children listing carries no version, so the page is read for it.
        let version = match &page.version {
            Some(version) => self.named(version).await?,
            None => {
                let read = self.read_page(&page.id).await?;
                self.named(read.version()?).await?
            }
        };
        Ok(RemoteItem {
            item: RemoteItemRef::Page {
                page_id: page.id.clone(),
            },
            title: page.title.clone(),
            path: page_path(prefix, &page.title),
            // Knowing would take a request per page, and every page may hold children or attachments, so
            // the walker always opens it.
            has_children: true,
            size_bytes: None,
            version,
        })
    }

    pub(crate) async fn attachment_item(
        &self,
        page_id: &str,
        attachment: &WireAttachment,
        prefix: &str,
    ) -> Result<RemoteItem, ConfluenceError> {
        Ok(RemoteItem {
            item: RemoteItemRef::Attachment {
                page_id: page_id.to_owned(),
                attachment_id: attachment.id.clone(),
            },
            title: attachment.title.clone(),
            path: join(prefix, &path_title(&attachment.title)),
            has_children: false,
            size_bytes: attachment.file_size,
            version: self.named(attachment.version()?).await?,
        })
    }

    /// Internal Logic 2.2 and 1.4: one provider page per call. For a page its child pages come first; the
    /// cursor after the last of them starts its attachments.
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-PAGE-01
    pub async fn list_children(
        &self,
        loc: &ConfluenceLocation,
        cursor: Option<Cursor>,
    ) -> Result<Page<RemoteItem>, ConfluenceError> {
        let url = match &cursor {
            Some(cursor) => {
                let link: String = cursor.decode()?;
                if !link.starts_with("/wiki/") {
                    return Err(ConfluenceError::BadCursor);
                }
                link
            }
            None => match &loc.page_id {
                Some(page_id) => page_children(page_id),
                None => Self::root_pages_url(&self.space_by_key(&loc.space_key).await?.id),
            },
        };
        let prefix = self.prefix_of(loc).await?;
        let path_only = url.split('?').next().unwrap_or_default();
        if path_only.ends_with("/attachments") {
            let page_id = loc.page_id.as_deref().ok_or(ConfluenceError::BadCursor)?;
            let listing: Listing<WireAttachment> = self
                .transport
                .call(Req::get("attachments", url))
                .await?
                .json()?;
            let mut items = Vec::with_capacity(listing.results.len());
            for attachment in &listing.results {
                items.push(self.attachment_item(page_id, attachment, &prefix).await?);
            }
            return Ok(Page {
                items,
                next_cursor: Self::cursor_of(listing.links.next.as_deref())?,
            });
        }
        let listing: Listing<WirePage> = self
            .transport
            .call(Req::get("children", url))
            .await?
            .json()?;
        let mut items = Vec::with_capacity(listing.results.len());
        for page in &listing.results {
            items.push(self.page_item(page, &prefix).await?);
        }
        let next_cursor = match (listing.links.next.as_deref(), &loc.page_id) {
            (None, Some(page_id)) => Self::cursor_of(Some(&page_attachments(page_id)))?,
            (next, _) => Self::cursor_of(next)?,
        };
        Ok(Page { items, next_cursor })
    }

    /// The caller's cursor wraps the next relative link. One too long to hand over is a bad answer.
    fn cursor_of(link: Option<&str>) -> Result<Option<Cursor>, ConfluenceError> {
        let Some(link) = link else {
            return Ok(None);
        };
        let cursor = Cursor::encode(&next_relative(link)?);
        cursor
            .decode::<String>()
            .map_err(|_| ConfluenceError::ResponseInvalid)?;
        Ok(Some(cursor))
    }

    /// Internal Logic 2.3: both re-read the version after the transfer and repeat once if it moved.
    pub async fn fetch(&self, item: &RemoteItemRef) -> Result<FetchedItem, ConfluenceError> {
        let mut last = None;
        for _ in 0..2 {
            let (path, bytes, before) = self.transfer(item).await?;
            let after = self.version_read(item).await?;
            if after.tag == before.tag {
                return Ok(FetchedItem {
                    item: item.clone(),
                    path,
                    bytes,
                    version: before,
                });
            }
            last = Some(after);
        }
        Err(match last {
            Some(current) => ConfluenceError::OriginChanged { current },
            None => ConfluenceError::Unavailable,
        })
    }

    async fn transfer(
        &self,
        item: &RemoteItemRef,
    ) -> Result<(String, Vec<u8>, RemoteVersion), ConfluenceError> {
        let known = store::known_path(self.pool()?, &item_key(item)).await?;
        match item {
            RemoteItemRef::Page { page_id } => {
                let page = self.read_page(page_id).await?;
                let body = page.storage_or_empty();
                if body.len() > MAX_PAGE_BODY_BYTES {
                    return Err(ConfluenceError::TooLarge);
                }
                let path = known.unwrap_or_else(|| page_path("", &page.title));
                Ok((
                    path,
                    body.as_bytes().to_vec(),
                    self.named(page.version()?).await?,
                ))
            }
            RemoteItemRef::Attachment {
                page_id,
                attachment_id,
            } => {
                let attachment = self.read_attachment(page_id, attachment_id).await?;
                if attachment.file_size.unwrap_or(0) > MAX_ATTACHMENT_BYTES {
                    return Err(ConfluenceError::TooLarge);
                }
                let link = attachment
                    .download_link
                    .as_deref()
                    .ok_or(ConfluenceError::ResponseInvalid)?;
                let reply = self
                    .transport
                    .call(Req::get("attachment_content", format!("/wiki{link}")).transfer())
                    .await?;
                if reply.body.len() as u64 > MAX_ATTACHMENT_BYTES {
                    return Err(ConfluenceError::TooLarge);
                }
                let path = match known {
                    Some(path) => path,
                    None => {
                        let page = self.read_page(page_id).await?;
                        join(&path_title(&page.title), &path_title(&attachment.title))
                    }
                };
                Ok((
                    path,
                    reply.body.to_vec(),
                    self.named(attachment.version()?).await?,
                ))
            }
        }
    }

    pub async fn item_version(
        &self,
        item: &RemoteItemRef,
    ) -> Result<RemoteVersion, ConfluenceError> {
        match item {
            RemoteItemRef::Page { page_id } => {
                let page = self.read_page(page_id).await?;
                self.named(page.version()?).await
            }
            RemoteItemRef::Attachment {
                page_id,
                attachment_id,
            } => {
                let attachment = self.read_attachment(page_id, attachment_id).await?;
                self.named(attachment.version()?).await
            }
        }
    }
}
