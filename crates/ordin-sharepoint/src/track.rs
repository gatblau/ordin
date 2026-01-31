//! Change tracking (Internal Logic 4; D-8; A-25): the drive's delta query from the library root with
//! `token=latest`, filtered to the link's folder path.

use std::collections::{HashMap, HashSet};

use ordin_types::domain::places::{SourceLink, SourceLinkKind};
use ordin_types::time::Timestamp;
use ordin_types::{CodedError as _, SourceLinkId};

use crate::adapter::{SharePointAdapter, MAX_FOLDER_LEVELS, MAX_LIST_ITEMS};
use crate::error::SharePointError;
use crate::graph::Req;
use crate::store::{self, ItemRow, LinkState};
use crate::types::{OriginChange, SharePointLocation};
use crate::wire::{join_path, DriveItem, ListPage};

/// *Assumed here*: a delta that never ends is cut after this many Graph pages.
const MAX_DELTA_PAGES: usize = 10_000;
const MAX_DELTA_LINK_CHARS: usize = 4096;
const MAX_ORIGIN_PATH_CHARS: usize = 2048;

impl SharePointAdapter {
    /// Internal Logic 4.1: resolves the link's location, takes the delta starting point and records what
    /// the location holds now.
    // spec: ordin-desktop/components/SharePointAdapter#SA-RECON-03
    pub async fn track_link(&self, link: &SourceLink) -> Result<(), SharePointError> {
        let (loc, delta_link, rows) = self.snapshot(link).await?;
        let state = state_of(&loc, delta_link);
        store::replace_tracking(
            &self.pool,
            link.id,
            &state,
            &rows,
            &[],
            Timestamp::now(),
            false,
        )
        .await?;
        tracing::info!(
            component = "sharepoint",
            source_link_id = link.id.to_string().as_str(),
            items = rows.len() as u64,
            "sharepoint_link_tracked"
        );
        Ok(())
    }

    /// Deletes the link's state row; its items and changes go with it (cascade).
    pub async fn untrack_link(&self, link: SourceLinkId) -> Result<(), SharePointError> {
        store::delete_link(&self.pool, link).await
    }

    /// Internal Logic 4.2 to 4.4.
    pub async fn check_changes(
        &self,
        link: &SourceLink,
    ) -> Result<Vec<OriginChange>, SharePointError> {
        let result = self.check_inner(link).await;
        if let Err(error) = &result {
            store::set_error(&self.pool, link.id, error.code()).await;
        }
        result
    }

    /// Internal Logic 4.5.
    pub async fn changes_since(
        &self,
        links: &[SourceLinkId],
        since: Timestamp,
    ) -> Result<Vec<OriginChange>, SharePointError> {
        store::changes_since(&self.pool, links, since).await
    }

    async fn check_inner(&self, link: &SourceLink) -> Result<Vec<OriginChange>, SharePointError> {
        let Some(state) = store::load_state(&self.pool, link.id).await? else {
            return Err(SharePointError::NotTracked);
        };
        let now = Timestamp::now();
        store::purge_old_changes(&self.pool, now).await?;

        let mut entries: Vec<DriveItem> = Vec::new();
        let mut url = state.delta_link.clone();
        let mut new_delta: Option<String> = None;
        for _ in 0..MAX_DELTA_PAGES {
            let reply = self
                .graph
                .call(Req::get("delta", self.graph.follow(&url)?).pass(&[410]))
                .await?;
            if reply.status == 410 {
                return self.resynchronise(link, now).await;
            }
            let page: ListPage<DriveItem> = reply.json()?;
            entries.extend(page.value);
            match (page.next_link, page.delta_link) {
                (Some(next), _) => url = next,
                (None, Some(delta)) => {
                    new_delta = Some(delta);
                    break;
                }
                (None, None) => return Err(SharePointError::ResponseInvalid),
            }
        }
        let new_delta = new_delta.ok_or(SharePointError::ResponseInvalid)?;
        let new_delta = self.checked_delta_link(&new_delta)?;

        let mut known = store::load_items(&self.pool, link.id).await?;
        let mut changes: Vec<OriginChange> = Vec::new();
        let mut upserts: Vec<ItemRow> = Vec::new();
        for entry in entries {
            self.apply_entry(
                link.id,
                &state,
                entry,
                &mut known,
                &mut upserts,
                &mut changes,
                now,
            )
            .await?;
        }
        store::commit_check(&self.pool, link.id, &new_delta, &upserts, &changes, now).await?;
        if !changes.is_empty() {
            tracing::info!(
                component = "sharepoint",
                source_link_id = link.id.to_string().as_str(),
                count = changes.len() as u64,
                "sharepoint_changes_found"
            );
        }
        Ok(changes)
    }

    /// One delta entry against the rows held (Internal Logic 4.2).
    #[allow(clippy::too_many_arguments)]
    async fn apply_entry(
        &self,
        link: SourceLinkId,
        state: &LinkState,
        entry: DriveItem,
        known: &mut HashMap<String, ItemRow>,
        upserts: &mut Vec<ItemRow>,
        changes: &mut Vec<OriginChange>,
        now: Timestamp,
    ) -> Result<(), SharePointError> {
        if entry.is_deleted() {
            if let Some(row) = known.get_mut(&entry.id) {
                if !row.removed {
                    row.removed = true;
                    changes.push(OriginChange {
                        source_link_id: link,
                        origin_path: row.origin_path.clone(),
                        new_version: String::new(),
                        changed_by: None,
                        changed_at: now,
                        removed: true,
                    });
                    upserts.push(row.clone());
                }
            }
            return Ok(());
        }
        if entry.is_folder() {
            return Ok(());
        }
        // Graph does not return `parentReference.path` for every delta entry on SharePoint. An
        // entry without it is placed by its metadata (one request); one that is gone by then counts
        // as outside the link.
        let full_path = match entry.full_path() {
            Some(path) => Some(path),
            None => match self.item_meta(&state.drive_id, Some(&entry.id)).await {
                Ok(meta) => meta.full_path(),
                Err(SharePointError::NotFound) => None,
                Err(e) => return Err(e),
            },
        };
        let relative = full_path.and_then(|p| relative_to(&state.path_prefix, &p));
        let Some(origin_path) = relative else {
            // Outside the link now: a known item that moved out is a removal.
            if let Some(row) = known.get_mut(&entry.id) {
                if !row.removed {
                    row.removed = true;
                    changes.push(OriginChange {
                        source_link_id: link,
                        origin_path: row.origin_path.clone(),
                        new_version: String::new(),
                        changed_by: entry.changed_by(),
                        changed_at: now,
                        removed: true,
                    });
                    upserts.push(row.clone());
                }
            }
            return Ok(());
        };
        if origin_path.chars().count() > MAX_ORIGIN_PATH_CHARS {
            return Ok(());
        }
        let version = entry.version(None)?;
        let changed = known
            .get(&entry.id)
            .is_none_or(|row| row.removed || row.ctag != version.tag);
        if changed {
            changes.push(OriginChange {
                source_link_id: link,
                origin_path: origin_path.clone(),
                new_version: version.tag.clone(),
                changed_by: version.changed_by.clone(),
                changed_at: version.changed_at,
                removed: false,
            });
        }
        let row = ItemRow {
            item_id: entry.id.clone(),
            origin_path,
            ctag: version.tag,
            etag: version.etag,
            version_label: None,
            changed_at: version.changed_at,
            changed_by: version.changed_by,
            removed: false,
        };
        if changed || known.get(&entry.id) != Some(&row) {
            known.insert(entry.id.clone(), row.clone());
            upserts.push(row);
        }
        Ok(())
    }

    /// Internal Logic 4.3: an expired delta token (410 Gone) walks the link again and reports every item
    /// whose cTag differs from the stored one.
    async fn resynchronise(
        &self,
        link: &SourceLink,
        now: Timestamp,
    ) -> Result<Vec<OriginChange>, SharePointError> {
        tracing::info!(
            component = "sharepoint",
            source_link_id = link.id.to_string().as_str(),
            "delta_resync"
        );
        let old = store::load_items(&self.pool, link.id).await?;
        let (loc, delta_link, rows) = self.snapshot(link).await?;
        let mut changes: Vec<OriginChange> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for row in &rows {
            seen.insert(row.item_id.as_str());
            let differs = old
                .get(&row.item_id)
                .is_none_or(|before| before.removed || before.ctag != row.ctag);
            if differs {
                changes.push(OriginChange {
                    source_link_id: link.id,
                    origin_path: row.origin_path.clone(),
                    new_version: row.ctag.clone(),
                    changed_by: row.changed_by.clone(),
                    changed_at: row.changed_at,
                    removed: false,
                });
            }
        }
        let mut all = rows.clone();
        for (id, before) in &old {
            if seen.contains(id.as_str()) {
                continue;
            }
            if !before.removed {
                changes.push(OriginChange {
                    source_link_id: link.id,
                    origin_path: before.origin_path.clone(),
                    new_version: String::new(),
                    changed_by: None,
                    changed_at: now,
                    removed: true,
                });
            }
            let mut gone = before.clone();
            gone.removed = true;
            all.push(gone);
        }
        let fresh = state_of(&loc, delta_link);
        store::replace_tracking(&self.pool, link.id, &fresh, &all, &changes, now, true).await?;
        if !changes.is_empty() {
            tracing::info!(
                component = "sharepoint",
                source_link_id = link.id.to_string().as_str(),
                count = changes.len() as u64,
                "sharepoint_changes_found"
            );
        }
        Ok(changes)
    }

    /// The delta starting point first, then the walk, so a change made during the walk is not missed.
    async fn snapshot(
        &self,
        link: &SourceLink,
    ) -> Result<(SharePointLocation, String, Vec<ItemRow>), SharePointError> {
        if link.kind != SourceLinkKind::SharepointLocation {
            return Err(SharePointError::LocationInvalid);
        }
        let loc = self.resolve_origin(&link.origin_location).await?;
        let delta_link = self.starting_point(&loc.drive_id).await?;
        let rows = self.walk(&loc).await?;
        Ok((loc, delta_link, rows))
    }

    /// `GET /drives/{drive-id}/root/delta?token=latest`, following `@odata.nextLink` to the delta link.
    async fn starting_point(&self, drive_id: &str) -> Result<String, SharePointError> {
        let mut url = self.graph.url(
            &["drives", drive_id, "root", "delta"],
            &[("token", "latest")],
        );
        for _ in 0..MAX_DELTA_PAGES {
            let page: ListPage<DriveItem> = self
                .graph
                .call(Req::get("delta_start", url))
                .await?
                .json()?;
            match (page.next_link, page.delta_link) {
                (Some(next), _) => url = self.graph.follow(&next)?,
                (None, Some(delta)) => return self.checked_delta_link(&delta),
                (None, None) => return Err(SharePointError::ResponseInvalid),
            }
        }
        Err(SharePointError::ResponseInvalid)
    }

    /// A delta link is kept only when it is a Graph address and fits the column.
    fn checked_delta_link(&self, link: &str) -> Result<String, SharePointError> {
        let followed = self.graph.follow(link)?;
        if followed.chars().count() > MAX_DELTA_LINK_CHARS {
            return Err(SharePointError::ResponseInvalid);
        }
        Ok(followed)
    }

    /// A full walk of the location: files only, at most `MAX_LIST_ITEMS` items and `MAX_FOLDER_LEVELS` levels.
    async fn walk(&self, loc: &SharePointLocation) -> Result<Vec<ItemRow>, SharePointError> {
        let mut rows: Vec<ItemRow> = Vec::new();
        if !loc.is_folder {
            let id = loc
                .item_id
                .as_deref()
                .ok_or(SharePointError::ResponseInvalid)?;
            let meta = self.item_meta(&loc.drive_id, Some(id)).await?;
            rows.push(row_of(&meta, meta.name.clone())?);
            return Ok(rows);
        }
        let mut queue: Vec<(Option<String>, String, usize)> =
            vec![(loc.item_id.clone(), String::new(), 1)];
        while let Some((folder, relative, depth)) = queue.pop() {
            let mut skiptoken: Option<String> = None;
            loop {
                let (children, next) = self
                    .children_page(&loc.drive_id, folder.as_deref(), skiptoken.as_deref())
                    .await?;
                for child in children {
                    let path = join_path(&relative, &child.name);
                    if child.is_folder() {
                        if depth < MAX_FOLDER_LEVELS {
                            queue.push((Some(child.id.clone()), path, depth + 1));
                        }
                    } else if path.chars().count() <= MAX_ORIGIN_PATH_CHARS {
                        rows.push(row_of(&child, path)?);
                    }
                    if rows.len() >= MAX_LIST_ITEMS {
                        return Ok(rows);
                    }
                }
                match next {
                    Some(token) => skiptoken = Some(token),
                    None => break,
                }
            }
        }
        Ok(rows)
    }
}

fn state_of(loc: &SharePointLocation, delta_link: String) -> LinkState {
    LinkState {
        drive_id: loc.drive_id.clone(),
        root_item_id: loc.item_id.clone().unwrap_or_else(|| "root".to_owned()),
        path_prefix: loc.path.clone(),
        delta_link,
    }
}

fn row_of(item: &DriveItem, origin_path: String) -> Result<ItemRow, SharePointError> {
    let version = item.version(None)?;
    Ok(ItemRow {
        item_id: item.id.clone(),
        origin_path,
        ctag: version.tag,
        etag: version.etag,
        version_label: None,
        changed_at: version.changed_at,
        changed_by: version.changed_by,
        removed: false,
    })
}

/// `full` relative to the link's `prefix`; a file link (`full == prefix`) is its own name; `None` outside.
fn relative_to(prefix: &str, full: &str) -> Option<String> {
    if prefix.is_empty() {
        return Some(full.to_owned());
    }
    if full == prefix {
        return full.rsplit('/').next().map(str::to_owned);
    }
    full.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('/'))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::relative_to;

    #[test]
    fn paths_are_placed_under_the_link_prefix() {
        assert_eq!(relative_to("", "a/b").as_deref(), Some("a/b"));
        assert_eq!(
            relative_to(
                "Suppliers/Contracts",
                "Suppliers/Contracts/Kestrel MSA.docx"
            )
            .as_deref(),
            Some("Kestrel MSA.docx")
        );
        assert_eq!(
            relative_to("Suppliers/Contracts", "Suppliers/Contracts/old/x.docx").as_deref(),
            Some("old/x.docx")
        );
        assert_eq!(
            relative_to("Suppliers/K.docx", "Suppliers/K.docx").as_deref(),
            Some("K.docx")
        );
        assert_eq!(
            relative_to("Suppliers/Contracts", "Suppliers/Contractsx/a"),
            None
        );
        assert_eq!(relative_to("Suppliers/Contracts", "Other/a"), None);
    }
}
