//! Change tracking (ConfluenceAdapter.md Internal Logic 3): what a link holds, what moved since the last
//! look, and the page or attachment behind a working-copy path.

use std::collections::{HashMap, HashSet};

use ordin_types::domain::places::SourceLink;
use ordin_types::time::Timestamp;
use ordin_types::SourceLinkId;

use crate::client::{next_relative, Req};
use crate::error::ConfluenceError;
use crate::read::search_url;
use crate::store::{self, ItemRow, LinkState};
use crate::types::{ConfluenceLocation, OriginChange, RemoteItem, RemoteItemRef};
use crate::wire::{
    cql_quote, is_page_id, item_key, item_of_key, join, page_path, page_prefix, path_title,
    Listing, SearchHit, WireAttachment, WirePage, PAGE_SUFFIX,
};
use crate::ConfluenceAdapter;

/// 10,000 pages per linked tree (A-21 scaled to pages).
const MAX_TREE_PAGES: usize = 10_000;
/// 20 levels per linked tree.
const MAX_TREE_LEVELS: usize = 20;
/// The overlap for clock skew on the change query (*chosen here*).
const CHECK_OVERLAP_MINUTES: i64 = 2;
/// `confluence_items.origin_path` is at most 2,048 characters.
const MAX_ORIGIN_PATH_CHARS: usize = 2048;

fn row_of(item: &RemoteItem) -> Result<ItemRow, ConfluenceError> {
    Ok(ItemRow {
        item_key: item_key(&item.item),
        origin_path: item.path.clone(),
        version_number: version_number(&item.version.tag)?,
        changed_at: item.version.changed_at,
        changed_by: item.version.changed_by.clone(),
        removed: false,
    })
}

pub(crate) fn version_number(tag: &str) -> Result<i64, ConfluenceError> {
    tag.parse().map_err(|_| ConfluenceError::ResponseInvalid)
}

/// The part of `lastmodified` the CQL form takes: `yyyy/MM/dd HH:mm`.
fn cql_time(at: Timestamp) -> String {
    at.0.format("%Y/%m/%d %H:%M").to_string()
}

fn is_direct_child(path: &str, parent_prefix: &str) -> bool {
    path.strip_prefix(parent_prefix)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|rest| !rest.contains('/') && rest.ends_with(PAGE_SUFFIX))
}

/// The working of one `check_changes`.
struct Check {
    link: SourceLinkId,
    rows: HashMap<String, ItemRow>,
    upserts: Vec<ItemRow>,
    changes: Vec<OriginChange>,
}

impl Check {
    fn removed(&mut self, key: &str, at: Timestamp) {
        let Some(row) = self.rows.get_mut(key) else {
            return;
        };
        if row.removed {
            return;
        }
        row.removed = true;
        self.changes.push(OriginChange {
            source_link_id: self.link,
            origin_path: row.origin_path.clone(),
            new_version: row.version_number.to_string(),
            changed_by: None,
            changed_at: at,
            removed: true,
        });
        self.upserts.push(row.clone());
    }

    /// A version above the stored one, or an item not seen before, is a change.
    fn observed(&mut self, item: ItemRow) {
        let moved = self
            .rows
            .get(&item.item_key)
            .is_none_or(|old| item.version_number > old.version_number || old.removed);
        if moved {
            self.changes.push(OriginChange {
                source_link_id: self.link,
                origin_path: item.origin_path.clone(),
                new_version: item.version_number.to_string(),
                changed_by: item.changed_by.clone(),
                changed_at: item.changed_at,
                removed: false,
            });
            self.rows.insert(item.item_key.clone(), item.clone());
            self.upserts.push(item);
        }
    }

    fn page_of_attachment(key: &str) -> Option<String> {
        match item_of_key(key) {
            Ok(RemoteItemRef::Attachment { page_id, .. }) => Some(page_id),
            _ => None,
        }
    }
}

impl ConfluenceAdapter {
    /// A breadth-first walk from `loc`: every page and attachment, at most `MAX_TREE_PAGES` pages and
    /// `MAX_TREE_LEVELS` levels.
    async fn walk(&self, loc: &ConfluenceLocation) -> Result<Vec<ItemRow>, ConfluenceError> {
        let mut rows: Vec<ItemRow> = Vec::new();
        let mut pages = 0usize;
        // (page id, its path prefix, its level)
        let mut queue: Vec<(WirePage, String, usize)> = Vec::new();
        match &loc.page_id {
            Some(page_id) => queue.push((self.read_page(page_id).await?, String::new(), 1)),
            None => {
                let space = self.space_by_key(&loc.space_key).await?;
                for page in self.root_pages(&space.id).await? {
                    queue.push((page, String::new(), 1));
                }
            }
        }
        let mut index = 0;
        while index < queue.len() && pages < MAX_TREE_PAGES {
            let (page, prefix, level) = &queue[index];
            let (level, prefix) = (*level, prefix.clone());
            let page_id = page.id.clone();
            let item = self.page_item(page, &prefix).await?;
            index += 1;
            pages += 1;
            if item.path.chars().count() <= MAX_ORIGIN_PATH_CHARS {
                rows.push(row_of(&item)?);
            }
            let inner = page_prefix(&item.path);
            for attachment in self.attachments(&page_id).await? {
                let item = self.attachment_item(&page_id, &attachment, &inner).await?;
                if item.path.chars().count() <= MAX_ORIGIN_PATH_CHARS {
                    rows.push(row_of(&item)?);
                }
            }
            if level < MAX_TREE_LEVELS {
                for child in self.children(&page_id).await? {
                    queue.push((child, inner.clone(), level + 1));
                }
            }
        }
        Ok(rows)
    }

    /// Internal Logic 3.1.
    // spec: ordin-desktop/components/ConfluenceAdapter#A-25
    pub async fn track_link(&self, link: &SourceLink) -> Result<(), ConfluenceError> {
        let loc = self.resolve_origin(&link.origin_location).await?;
        let rows = self.walk(&loc).await?;
        let now = Timestamp::now();
        store::replace_tracking(
            self.pool()?,
            link.id,
            &LinkState {
                space_key: loc.space_key,
                root_page_id: loc.page_id,
                last_checked_at: now,
            },
            &rows,
            now,
        )
        .await
    }

    pub async fn untrack_link(&self, link: SourceLinkId) -> Result<(), ConfluenceError> {
        store::delete_link(self.pool()?, link).await
    }

    /// Internal Logic 3.4: no request to Confluence.
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-09
    pub async fn item_for_path(
        &self,
        link: SourceLinkId,
        origin_path: &str,
    ) -> Result<RemoteItemRef, ConfluenceError> {
        let pool = self.pool()?;
        store::load_state(pool, link)
            .await?
            .ok_or(ConfluenceError::NotTracked)?;
        let key = store::item_at_path(pool, link, origin_path)
            .await?
            .ok_or(ConfluenceError::NotFound)?;
        item_of_key(&key)
    }

    /// Every page row of the change query, following `_links.next`.
    async fn changed_pages(&self, state: &LinkState) -> Result<Vec<String>, ConfluenceError> {
        let since =
            Timestamp(state.last_checked_at.0 - chrono::Duration::minutes(CHECK_OVERLAP_MINUTES));
        let scope = match &state.root_page_id {
            Some(root) if is_page_id(root) => format!("(ancestor={root} OR id={root})"),
            Some(_) => return Err(ConfluenceError::LocationInvalid),
            None => format!("space={}", cql_quote(&state.space_key)),
        };
        let cql = format!("{scope} AND lastmodified >= \"{}\"", cql_time(since));
        let mut url = search_url(&cql, "100");
        let mut pages = Vec::new();
        loop {
            let listing: Listing<SearchHit> = self
                .transport
                .call(Req::get("changes", url))
                .await?
                .json()?;
            pages.extend(
                listing
                    .results
                    .into_iter()
                    .filter_map(|hit| hit.content)
                    .filter(|content| content.kind == "page")
                    .map(|content| content.id),
            );
            match listing.links.next {
                Some(next) => url = next_relative(&next)?,
                None => return Ok(pages),
            }
        }
    }

    /// The prefix a new page belongs under: its parent's stored path, or the link root.
    fn prefix_for(check: &Check, page: &WirePage) -> String {
        page.parent_id
            .as_ref()
            .and_then(|parent| check.rows.get(&format!("page:{parent}")))
            .map(|row| page_prefix(&row.origin_path))
            .unwrap_or_default()
    }

    /// One page of the change query: its version, its attachments, and the children it still lists.
    async fn check_page(&self, check: &mut Check, page_id: &str) -> Result<(), ConfluenceError> {
        let key = format!("page:{page_id}");
        let page = match self.read_page(page_id).await {
            Ok(page) => page,
            Err(ConfluenceError::NotFound) => {
                self.page_gone(check, &key);
                return Ok(());
            }
            Err(other) => return Err(other),
        };
        let stored = check.rows.get(&key).map(|row| row.origin_path.clone());
        let path =
            stored.unwrap_or_else(|| page_path(&Self::prefix_for(check, &page), &page.title));
        let prefix = page_prefix(&path);
        let wire = page.version()?;
        let remote = wire.remote()?;
        check.observed(ItemRow {
            item_key: key.clone(),
            origin_path: path,
            version_number: version_number(&remote.tag)?,
            changed_at: remote.changed_at,
            changed_by: self.transport.author(wire.account()).await,
            removed: false,
        });

        let attachments = self.attachments(page_id).await?;
        let listed: HashSet<String> = attachments
            .iter()
            .map(|a: &WireAttachment| format!("attachment:{page_id}:{}", a.id))
            .collect();
        for attachment in &attachments {
            let wire = attachment.version()?;
            let remote = wire.remote()?;
            let key = format!("attachment:{page_id}:{}", attachment.id);
            let path = check.rows.get(&key).map_or_else(
                || join(&prefix, &path_title(&attachment.title)),
                |row| row.origin_path.clone(),
            );
            check.observed(ItemRow {
                item_key: key,
                origin_path: path,
                version_number: version_number(&remote.tag)?,
                changed_at: remote.changed_at,
                changed_by: self.transport.author(wire.account()).await,
                removed: false,
            });
        }
        let now = Timestamp::now();
        let gone: Vec<String> = check
            .rows
            .keys()
            .filter(|key| {
                Check::page_of_attachment(key).as_deref() == Some(page_id) && !listed.contains(*key)
            })
            .cloned()
            .collect();
        for key in gone {
            check.removed(&key, now);
        }

        let children = self.children(page_id).await?;
        let listed: HashSet<String> = children.iter().map(|c| format!("page:{}", c.id)).collect();
        let gone: Vec<String> = check
            .rows
            .values()
            .filter(|row| {
                row.item_key.starts_with("page:")
                    && !listed.contains(&row.item_key)
                    && is_direct_child(&row.origin_path, &prefix)
            })
            .map(|row| row.item_key.clone())
            .collect();
        for key in gone {
            self.page_gone(check, &key);
        }
        Ok(())
    }

    /// A page gone from Confluence takes its attachments with it.
    fn page_gone(&self, check: &mut Check, key: &str) {
        let now = Timestamp::now();
        check.removed(key, now);
        let page_id = key.strip_prefix("page:").unwrap_or_default();
        let attachments: Vec<String> = check
            .rows
            .keys()
            .filter(|k| Check::page_of_attachment(k).as_deref() == Some(page_id))
            .cloned()
            .collect();
        for attachment in attachments {
            check.removed(&attachment, now);
        }
    }

    /// Internal Logic 3.2.
    pub async fn check_changes(
        &self,
        link: &SourceLink,
    ) -> Result<Vec<OriginChange>, ConfluenceError> {
        let pool = self.pool()?;
        let state = store::load_state(pool, link.id)
            .await?
            .ok_or(ConfluenceError::NotTracked)?;
        let now = Timestamp::now();
        let checked = self.check_tree(link.id, &state, now).await;
        let changes = match checked {
            Ok(changes) => changes,
            Err(e) => {
                store::set_error(pool, link.id, e.code()).await;
                return Err(e);
            }
        };
        store::purge_old_changes(pool, now).await?;
        tracing::info!(
            component = "confluence",
            source_link_id = link.id.to_string().as_str(),
            count = changes.len() as u64,
            "confluence_changes_found"
        );
        Ok(changes)
    }

    async fn check_tree(
        &self,
        link: SourceLinkId,
        state: &LinkState,
        now: Timestamp,
    ) -> Result<Vec<OriginChange>, ConfluenceError> {
        let pool = self.pool()?;
        let pages = self.changed_pages(state).await?;
        let mut check = Check {
            link,
            rows: store::load_items(pool, link).await?,
            upserts: Vec::new(),
            changes: Vec::new(),
        };
        for page_id in pages {
            if is_page_id(&page_id) {
                self.check_page(&mut check, &page_id).await?;
            }
        }
        store::commit_check(pool, link, &check.upserts, &check.changes, now).await?;
        Ok(check.changes)
    }

    /// Internal Logic 3.3: rows found after `since`, newest first, at most 1,000.
    pub async fn changes_since(
        &self,
        links: &[SourceLinkId],
        since: Timestamp,
    ) -> Result<Vec<OriginChange>, ConfluenceError> {
        store::changes_since(self.pool()?, links, since).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::parse_time;

    #[test]
    fn a_time_is_written_as_the_cql_form() {
        let at = Timestamp(parse_time("2026-01-10T09:05:30.000Z").unwrap().0);
        assert_eq!(cql_time(at), "2026/01/10 09:05");
    }

    #[test]
    fn only_a_direct_child_page_counts() {
        assert!(is_direct_child("A/B.confluence.xhtml", "A"));
        assert!(!is_direct_child("A/B/C.confluence.xhtml", "A"));
        assert!(!is_direct_child("A/b.pdf", "A"));
        assert!(!is_direct_child("AB/C.confluence.xhtml", "A"));
    }

    #[test]
    fn a_tag_that_is_no_number_is_a_bad_response() {
        assert_eq!(
            version_number("x").unwrap_err().code(),
            "CONF_RESPONSE_INVALID"
        );
    }
}
