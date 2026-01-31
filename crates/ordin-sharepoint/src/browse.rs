//! Finding and listing (Internal Logic 2) and the paging cursor (Internal Logic 1.4).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ordin_types::paging::CURSOR_MAX_CHARS;
use ordin_types::{Cursor, Page};
use serde::{Deserialize, Serialize};

use crate::adapter::{SharePointAdapter, FIND_LIMIT};
use crate::error::SharePointError;
use crate::graph::Req;
use crate::types::{SharePointItem, SharePointItemRef, SharePointLocation};
use crate::wire::{Drive, DriveItem, ListPage, Site, CHILDREN_SELECT, ITEM_SELECT};

/// What a `list_children` cursor holds (*chosen here*): no address, so it cannot redirect a request.
#[derive(Debug, Serialize, Deserialize)]
struct ChildrenCursor {
    drive_id: String,
    item_id: String,
    skiptoken: String,
}

/// The `item_id` a cursor records for the library root.
const ROOT: &str = "root";
/// Sites searched and libraries searched for folders per call (Internal Logic 2.1).
const MAX_SITES: usize = 10;
const MAX_LIBRARIES: usize = 10;
const MAX_QUERY_CHARS: usize = 200;
/// The prefix of the stored origin form.
const ORIGIN_PREFIX: &str = "graph:drives/";

impl SharePointAdapter {
    /// Internal Logic 2.1.
    pub async fn find_locations(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<SharePointLocation>, SharePointError> {
        let query = query.trim();
        if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
            return Err(SharePointError::QueryInvalid);
        }
        let limit = limit.clamp(1, FIND_LIMIT) as usize;

        // Sites, following `@odata.nextLink` until ten are held or the list ends.
        let mut sites: Vec<Site> = Vec::new();
        let mut url = self.graph.url(
            &["sites"],
            &[("search", query), ("$select", "id,displayName,webUrl")],
        );
        loop {
            let page: ListPage<Site> =
                self.graph.call(Req::get("find_sites", url)).await?.json()?;
            sites.extend(page.value);
            match page.next_link {
                Some(link) if sites.len() < MAX_SITES => url = self.graph.follow(&link)?,
                _ => break,
            }
        }
        sites.truncate(MAX_SITES);

        // Libraries of each site.
        let mut libraries: Vec<(usize, Drive)> = Vec::new();
        for (index, site) in sites.iter().enumerate() {
            let mut url = self.graph.url(
                &["sites", &site.id, "drives"],
                &[("$select", "id,name,webUrl")],
            );
            loop {
                let page: ListPage<Drive> = self
                    .graph
                    .call(Req::get("find_libraries", url))
                    .await?
                    .json()?;
                libraries.extend(page.value.into_iter().map(|d| (index, d)));
                match page.next_link {
                    Some(link) => url = self.graph.follow(&link)?,
                    None => break,
                }
            }
        }

        let needle = query.to_lowercase();
        let mut library_hits: Vec<SharePointLocation> = libraries
            .iter()
            .filter(|(_, drive)| drive.name.to_lowercase().contains(&needle))
            .map(|(index, drive)| SharePointLocation {
                site_id: sites[*index].id.clone(),
                site_name: sites[*index].display_name.clone(),
                drive_id: drive.id.clone(),
                drive_name: drive.name.clone(),
                item_id: None,
                path: String::new(),
                is_folder: true,
                web_url: drive.web_url.clone().unwrap_or_default(),
            })
            .collect();
        library_hits.sort_by_key(|l| l.drive_name.to_lowercase());

        // Folders: up to ten libraries in all, from `root/search` with a `folder` facet.
        let mut folder_hits: Vec<SharePointLocation> = Vec::new();
        for (index, drive) in libraries.iter().take(MAX_LIBRARIES) {
            let search = format!("search(q='{}')", query.replace('\'', "''"));
            let url = self.graph.url(
                &["drives", &drive.id, "root", &search],
                &[
                    ("$select", "id,name,folder,parentReference,webUrl"),
                    ("$top", "50"),
                ],
            );
            let page: ListPage<DriveItem> = self
                .graph
                .call(Req::get("find_folders", url))
                .await?
                .json()?;
            for item in page.value.into_iter().filter(DriveItem::is_folder) {
                folder_hits.push(SharePointLocation {
                    site_id: sites[*index].id.clone(),
                    site_name: sites[*index].display_name.clone(),
                    drive_id: drive.id.clone(),
                    drive_name: drive.name.clone(),
                    path: item.full_path().unwrap_or_else(|| item.name.clone()),
                    item_id: Some(item.id),
                    is_folder: true,
                    web_url: item.web_url.unwrap_or_default(),
                });
            }
        }
        folder_hits.sort_by_key(|l| l.path.to_lowercase());

        library_hits.extend(folder_hits);
        library_hits.truncate(limit);
        Ok(library_hits)
    }

    /// The location form stored in `SourceLink.origin_location`, and back (`resolve_origin`).
    pub fn origin_location(loc: &SharePointLocation) -> String {
        format!(
            "{ORIGIN_PREFIX}{}/items/{}",
            loc.drive_id,
            loc.item_id.as_deref().unwrap_or(ROOT)
        )
    }

    /// Internal Logic 2.2.
    pub async fn resolve_origin(
        &self,
        origin_location: &str,
    ) -> Result<SharePointLocation, SharePointError> {
        let (drive_id, item_id) = parse_origin(origin_location)?;
        let item = self
            .item_meta(
                &drive_id,
                if item_id == ROOT {
                    None
                } else {
                    Some(&item_id)
                },
            )
            .await?;
        self.location_of(&item, &drive_id).await
    }

    /// Internal Logic 3a.
    pub async fn resolve_web_url(
        &self,
        web_url: &str,
    ) -> Result<SharePointLocation, SharePointError> {
        let parsed = url::Url::parse(web_url).map_err(|_| SharePointError::LocationInvalid)?;
        let host_ok = parsed
            .host_str()
            .is_some_and(|h| h.ends_with(".sharepoint.com"));
        if web_url.len() > 2048 || parsed.scheme() != "https" || !host_ok {
            return Err(SharePointError::LocationInvalid);
        }
        let share = format!("u!{}", URL_SAFE_NO_PAD.encode(web_url));
        let item: DriveItem = self
            .graph
            .call(Req::get(
                "resolve_web_url",
                self.graph.url(
                    &["shares", &share, "driveItem"],
                    &[("$select", ITEM_SELECT)],
                ),
            ))
            .await?
            .json()?;
        let drive_id = item
            .parent
            .as_ref()
            .and_then(|p| p.drive_id.clone())
            .ok_or(SharePointError::ResponseInvalid)?;
        self.location_of(&item, &drive_id).await
    }

    /// Internal Logic 2.3 and 1.4: one Graph page of `$top=100` per call.
    pub async fn list_children(
        &self,
        loc: &SharePointLocation,
        cursor: Option<Cursor>,
    ) -> Result<Page<SharePointItem>, SharePointError> {
        if !loc.is_folder {
            return Err(SharePointError::LocationInvalid);
        }
        let item_key = loc.item_id.as_deref().unwrap_or(ROOT);
        let skiptoken = match cursor {
            None => None,
            Some(cursor) => {
                let held: ChildrenCursor = cursor.decode()?;
                if held.drive_id != loc.drive_id || held.item_id != item_key {
                    return Err(SharePointError::BadCursor);
                }
                Some(held.skiptoken)
            }
        };
        let (children, next) = self
            .children_page(&loc.drive_id, loc.item_id.as_deref(), skiptoken.as_deref())
            .await?;
        let next_cursor = match next {
            None => None,
            Some(skiptoken) => {
                let cursor = Cursor::encode(&ChildrenCursor {
                    drive_id: loc.drive_id.clone(),
                    item_id: item_key.to_owned(),
                    skiptoken,
                });
                // A cursor that would exceed the limit is refused rather than sent (Internal Logic 1.4).
                let length = serde_json::to_value(&cursor)
                    .ok()
                    .and_then(|v| v.as_str().map(|s| s.chars().count()))
                    .unwrap_or(usize::MAX);
                if length > CURSOR_MAX_CHARS {
                    return Err(SharePointError::BadCursor);
                }
                Some(cursor)
            }
        };
        let mut items = Vec::with_capacity(children.len());
        for child in children {
            let is_folder = child.is_folder();
            items.push(SharePointItem {
                item: SharePointItemRef {
                    drive_id: loc.drive_id.clone(),
                    item_id: child.id.clone(),
                },
                path: if is_folder {
                    format!("{}/", child.name)
                } else {
                    child.name.clone()
                },
                name: child.name.clone(),
                is_folder,
                size_bytes: child.size.unwrap_or(0),
                // The label is not read per item while listing (one request per item).
                version: if is_folder {
                    None
                } else {
                    Some(child.version(None)?)
                },
            });
        }
        Ok(Page { items, next_cursor })
    }

    /// One Graph page of children and the `$skiptoken` of its `@odata.nextLink`.
    pub(crate) async fn children_page(
        &self,
        drive_id: &str,
        item_id: Option<&str>,
        skiptoken: Option<&str>,
    ) -> Result<(Vec<DriveItem>, Option<String>), SharePointError> {
        let mut query = vec![("$select", CHILDREN_SELECT), ("$top", "100")];
        if let Some(token) = skiptoken {
            query.push(("$skiptoken", token));
        }
        let url = match item_id {
            Some(id) => self
                .graph
                .url(&["drives", drive_id, "items", id, "children"], &query),
            None => self
                .graph
                .url(&["drives", drive_id, "root", "children"], &query),
        };
        let page: ListPage<DriveItem> = self
            .graph
            .call(Req::get("list_children", url))
            .await?
            .json()?;
        let next = match page.next_link {
            None => None,
            Some(link) => Some(skiptoken_of(&link)?),
        };
        Ok((page.value, next))
    }
}

/// The `$skiptoken` query value of a `@odata.nextLink`; a link without one is `SP_RESPONSE_INVALID`.
fn skiptoken_of(link: &str) -> Result<String, SharePointError> {
    let parsed = url::Url::parse(link).map_err(|_| SharePointError::ResponseInvalid)?;
    parsed
        .query_pairs()
        .find(|(key, _)| key == "$skiptoken")
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty())
        .ok_or(SharePointError::ResponseInvalid)
}

/// `graph:drives/<drive-id>/items/<item-id>` (1–2,048 bytes), else `SP_LOCATION_INVALID`.
fn parse_origin(origin: &str) -> Result<(String, String), SharePointError> {
    if origin.is_empty() || origin.len() > 2048 {
        return Err(SharePointError::LocationInvalid);
    }
    let rest = origin
        .strip_prefix(ORIGIN_PREFIX)
        .ok_or(SharePointError::LocationInvalid)?;
    let (drive, item) = rest
        .split_once("/items/")
        .ok_or(SharePointError::LocationInvalid)?;
    let clean = |s: &str| !s.is_empty() && !s.contains('/');
    if clean(drive) && clean(item) {
        Ok((drive.to_owned(), item.to_owned()))
    } else {
        Err(SharePointError::LocationInvalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_forms_round_trip_and_malformed_ones_are_refused() {
        let (drive, item) = parse_origin("graph:drives/b!x9Fq/items/01ABCDEF2GHIJ").unwrap();
        assert_eq!((drive.as_str(), item.as_str()), ("b!x9Fq", "01ABCDEF2GHIJ"));
        for bad in [
            "",
            "drives/a/items/b",
            "graph:drives/a",
            "graph:drives//items/b",
            "graph:drives/a/items/",
            "graph:drives/a/b/items/c",
            "graph:drives/a/items/b/c",
        ] {
            assert!(parse_origin(bad).is_err(), "{bad}");
        }
        assert!(parse_origin(&format!("graph:drives/a/items/{}", "x".repeat(2048))).is_err());
    }

    #[test]
    fn a_next_link_must_carry_a_skiptoken() {
        assert_eq!(
            skiptoken_of("https://graph.example/v1.0/drives/d/items/i/children?$skiptoken=abc%3D")
                .unwrap(),
            "abc="
        );
        assert!(skiptoken_of("https://graph.example/v1.0/x?$top=100").is_err());
        assert!(skiptoken_of("not a url").is_err());
    }

    #[test]
    fn root_origin_uses_the_root_marker() {
        let loc = SharePointLocation {
            site_id: "s".into(),
            site_name: "S".into(),
            drive_id: "d1".into(),
            drive_name: "Documents".into(),
            item_id: None,
            path: String::new(),
            is_folder: true,
            web_url: String::new(),
        };
        assert_eq!(
            SharePointAdapter::origin_location(&loc),
            "graph:drives/d1/items/root"
        );
    }
}
