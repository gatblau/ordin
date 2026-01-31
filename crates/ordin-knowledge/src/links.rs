//! Page links (amendment January 2026, ADR-104): the link graph (§P), renames' rewrites of
//! linking pages (§Q steps 2–3) and links on read (§T). ADR-112 keeps each rewritten link's form.

use std::collections::HashSet;

use ordin_files::{FileError, FileVersionOrigin, WriteVersion};
use ordin_types::domain::places::KnowledgePageState;
use ordin_types::time::Timestamp;
use ordin_types::{KnowledgePageId, WorkspaceId};
use serde_json::json;
use sqlx::Row;

use crate::error::KnowledgeError;
use crate::markdown::{resolve_bundle_path, scan_links, LinkForm, RawLink};
use crate::model::{PageLinkView, PageRef};
use crate::names::{below_knowledge, folder_of, name_key, path_stem};
use crate::okf;
use crate::service::{
    KnowledgeService, MAX_LINKS_PER_PAGE, RENAME_REWRITE_DELAY, REWRITE_ATTEMPTS,
};
use crate::store::{self, Conn, PageRow};

/// One live page of the workspace as link resolution needs it.
#[derive(Debug, Clone)]
pub(crate) struct PageName {
    pub id: KnowledgePageId,
    pub path: String,
}

pub(crate) async fn page_names(
    conn: &mut Conn,
    workspace: WorkspaceId,
) -> Result<Vec<PageName>, KnowledgeError> {
    let rows = sqlx::query(
        "SELECT id, relative_path, title FROM knowledge_pages WHERE workspace_id = ?1 AND state <> 'removed' \
         ORDER BY relative_path",
    )
    .bind(workspace.to_string())
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|r| {
            let id: String = r.try_get("id")?;
            Ok(PageName {
                id: KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?,
                path: r.try_get("relative_path")?,
            })
        })
        .collect()
}

/// The text a link's target is kept under (`target_text`): the wiki target, or a Markdown link's
/// path below the bundle root without `.md`. `None` when the link is not kept (§P step 3).
pub(crate) fn link_target(linking_path: &str, link: &RawLink) -> Option<String> {
    match link.form {
        LinkForm::Markdown => {
            let folder = folder_of(&below_knowledge(linking_path)).to_owned();
            let folder = if below_knowledge(linking_path).contains('/') {
                folder
            } else {
                String::new()
            };
            resolve_bundle_path(&folder, &link.target)
        }
        _ => Some(link.target.clone()),
    }
}

/// §P step 4: a target holding `/` names a path; else a file stem (one → it; several → the one
/// in the linking page's folder, else the shortest path, else the first); else a title or alias.
pub(crate) async fn resolve_target(
    conn: &mut Conn,
    workspace: WorkspaceId,
    linking_path: &str,
    target: &str,
    pages: &[PageName],
) -> Result<Option<KnowledgePageId>, KnowledgeError> {
    let key = name_key(target);
    if target.contains('/') {
        return Ok(pages
            .iter()
            .find(|p| name_key(&below_knowledge(&p.path)) == key)
            .map(|p| p.id));
    }
    let by_stem: Vec<&PageName> = pages
        .iter()
        .filter(|p| name_key(&path_stem(&p.path)) == key)
        .collect();
    match by_stem.len() {
        0 => {}
        1 => return Ok(Some(by_stem[0].id)),
        _ => {
            let folder = folder_of(linking_path);
            if let Some(p) = by_stem.iter().find(|p| folder_of(&p.path) == folder) {
                return Ok(Some(p.id));
            }
            let mut sorted = by_stem.clone();
            sorted.sort_by(|a, b| {
                a.path
                    .len()
                    .cmp(&b.path.len())
                    .then_with(|| a.path.cmp(&b.path))
            });
            return Ok(sorted.first().map(|p| p.id));
        }
    }
    store::name_holder(conn, workspace, &key).await
}

/// The name keys that named a page before a rename: its title, its stem and `<folder>/<stem>`.
pub(crate) fn old_keys(old: &PageRow) -> Vec<String> {
    let mut keys = vec![
        name_key(&old.title),
        name_key(&path_stem(&old.relative_path)),
    ];
    let below = below_knowledge(&old.relative_path);
    if below.contains('/') {
        keys.push(name_key(&below));
    }
    keys.sort();
    keys.dedup();
    keys
}

pub(crate) async fn unresolve_target(
    conn: &mut Conn,
    target: KnowledgePageId,
) -> Result<(), KnowledgeError> {
    // ADR-111 F: every page linking to a removed page is re-indexed, so the closed link reaches memory.
    sqlx::query(
        "UPDATE knowledge_pages SET memory_indexed_version = NULL, memory_next_attempt_at = NULL \
         WHERE id IN (SELECT page_id FROM knowledge_links WHERE target_page_id = ?1 AND page_id <> ?1)",
    )
    .bind(target.to_string())
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE knowledge_links SET target_page_id = NULL WHERE target_page_id = ?1")
        .bind(target.to_string())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// T-1: the person's `links_out` (body order) and `links_in` (each page once, by title).
pub(crate) async fn person_links(
    conn: &mut Conn,
    row: &PageRow,
) -> Result<(Vec<PageLinkView>, Vec<PageRef>), KnowledgeError> {
    let out = sqlx::query(
        "SELECT l.target_text, l.section, p.id AS target_id, p.title AS target_title \
         FROM knowledge_links l LEFT JOIN knowledge_pages p ON p.id = l.target_page_id AND p.state <> 'removed' \
         WHERE l.page_id = ?1 ORDER BY l.ordinal",
    )
    .bind(row.id.to_string())
    .fetch_all(&mut *conn)
    .await?;
    let mut links_out = Vec::with_capacity(out.len());
    for r in &out {
        let id: Option<String> = r.try_get("target_id")?;
        let title: Option<String> = r.try_get("target_title")?;
        links_out.push(PageLinkView {
            target: match (id, title) {
                (Some(i), Some(t)) => Some(PageRef {
                    page_id: KnowledgePageId::parse(&i).map_err(|_| store::corrupt())?,
                    title: t,
                }),
                _ => None,
            },
            name: r.try_get("target_text")?,
            section: r.try_get("section")?,
        });
    }
    let links_in = links_in(conn, row.id).await?;
    Ok((links_out, links_in))
}

pub(crate) async fn links_in(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<Vec<PageRef>, KnowledgeError> {
    let rows = sqlx::query(
        "SELECT DISTINCT p.id, p.title FROM knowledge_links l JOIN knowledge_pages p ON p.id = l.page_id \
         WHERE l.target_page_id = ?1 AND l.page_id <> ?1 AND p.state <> 'removed' ORDER BY p.title, p.id",
    )
    .bind(page.to_string())
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|r| {
            let id: String = r.try_get("id")?;
            Ok(PageRef {
                page_id: KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?,
                title: r.try_get("title")?,
            })
        })
        .collect()
}

/// A rewritten link (SA-KLINK-05 for a wiki link, SA-OKF-11 for a Markdown link).
fn rewritten(link: &RawLink, target: &PageRow, stem_shared: bool) -> String {
    match link.form {
        LinkForm::Markdown => okf::link_to(
            &below_knowledge(&target.relative_path),
            link.section.as_deref(),
            link.shown.as_deref().unwrap_or(&target.title),
        ),
        LinkForm::Wiki | LinkForm::Embed => {
            let stem = if stem_shared {
                below_knowledge(&target.relative_path)
            } else {
                path_stem(&target.relative_path)
            };
            let shown = link.shown.clone().unwrap_or_else(|| target.title.clone());
            let mut inner = stem.clone();
            if let Some(s) = &link.section {
                inner.push('#');
                inner.push_str(s);
            }
            if shown != stem {
                inner.push('|');
                inner.push_str(&shown);
            }
            let bang = if link.form == LinkForm::Embed {
                "!"
            } else {
                ""
            };
            format!("{bang}[[{inner}]]")
        }
    }
}

impl KnowledgeService {
    /// §P steps 1–5 for one page (run in the caller's transaction).
    pub(crate) async fn rebuild_links(
        &self,
        conn: &mut Conn,
        row: &mut PageRow,
    ) -> Result<(), KnowledgeError> {
        sqlx::query("DELETE FROM knowledge_links WHERE page_id = ?1")
            .bind(row.id.to_string())
            .execute(&mut *conn)
            .await?;
        let pages = page_names(conn, row.workspace_id).await?;
        let mut kept = 0usize;
        let mut truncated = 0u32;
        let mut unresolved = 0u32;
        if row.state != KnowledgePageState::Removed {
            for link in scan_links(&row.body) {
                let Some(target) = link_target(&row.relative_path, &link) else {
                    continue;
                };
                if kept >= MAX_LINKS_PER_PAGE {
                    truncated += 1;
                    continue;
                }
                kept += 1;
                let resolved =
                    resolve_target(conn, row.workspace_id, &row.relative_path, &target, &pages)
                        .await?;
                if resolved.is_none() {
                    unresolved += 1;
                }
                sqlx::query(
                    "INSERT INTO knowledge_links (page_id, ordinal, workspace_id, target_page_id, target_key, \
                     target_text, section, form, start_byte, end_byte) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )
                .bind(row.id.to_string())
                .bind(i64::try_from(kept).unwrap_or(i64::MAX))
                .bind(row.workspace_id.to_string())
                .bind(resolved.map(|r| r.to_string()))
                .bind(name_key(&target))
                .bind(&target)
                .bind(&link.section)
                .bind(link.form.text())
                .bind(i64::try_from(link.target_span.start).unwrap_or(0))
                .bind(i64::try_from(link.target_span.end.max(link.target_span.start + 1)).unwrap_or(1))
                .execute(&mut *conn)
                .await?;
            }
        }
        row.links_truncated = truncated;
        row.links_built_version = Some(row.file_version);
        sqlx::query("UPDATE knowledge_pages SET links_truncated = ?2, links_built_version = ?3 WHERE id = ?1")
            .bind(row.id.to_string())
            .bind(i64::from(truncated))
            .bind(i64::from(row.file_version))
            .execute(&mut *conn)
            .await?;
        tracing::info!(
            component = "knowledge",
            page_id = row.id.to_string().as_str(),
            links = kept,
            unresolved,
            "knowledge_links_built"
        );
        Ok(())
    }

    /// §P step 6: unresolved links of the workspace naming one of the page's names now resolve
    /// to it; nothing is rewritten.
    pub(crate) async fn resolve_names(
        &self,
        conn: &mut Conn,
        row: &PageRow,
    ) -> Result<(), KnowledgeError> {
        if row.state == KnowledgePageState::Removed {
            return Ok(());
        }
        let mut keys: Vec<String> = store::registered_keys(conn, row.id).await?;
        keys.push(name_key(&path_stem(&row.relative_path)));
        keys.push(name_key(&below_knowledge(&row.relative_path)));
        let keys = store::to_json(&keys);
        sqlx::query(
            "UPDATE knowledge_pages SET memory_indexed_version = NULL, memory_next_attempt_at = NULL WHERE id IN \
             (SELECT page_id FROM knowledge_links WHERE workspace_id = ?1 AND target_page_id IS NULL \
              AND target_key IN (SELECT value FROM json_each(?2)))",
        )
        .bind(row.workspace_id.to_string())
        .bind(&keys)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE knowledge_links SET target_page_id = ?3 WHERE workspace_id = ?1 AND target_page_id IS NULL \
             AND target_key IN (SELECT value FROM json_each(?2))",
        )
        .bind(row.workspace_id.to_string())
        .bind(&keys)
        .bind(row.id.to_string())
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// SA-KLINK-09 / §Q step 3: a rewrite due after `RENAME_REWRITE_DELAY`.
    pub(crate) async fn schedule_rewrite(
        &self,
        conn: &mut Conn,
        target: KnowledgePageId,
        keys: &[String],
    ) -> Result<(), KnowledgeError> {
        let due = Timestamp(
            Timestamp::now().0
                + chrono::Duration::from_std(RENAME_REWRITE_DELAY)
                    .unwrap_or(chrono::Duration::seconds(10)),
        );
        for k in keys {
            sqlx::query(
                "INSERT INTO knowledge_link_rewrites (target_page_id, old_key, due_at) VALUES (?1, ?2, ?3) \
                 ON CONFLICT (target_page_id, old_key) DO UPDATE SET due_at = excluded.due_at",
            )
            .bind(target.to_string())
            .bind(k)
            .bind(due.to_wire())
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    /// §Q step 3: every pending rewrite whose `due_at` has passed.
    pub(crate) async fn run_due_rewrites(&self, now: Timestamp) -> Result<(), KnowledgeError> {
        if self.program.is_agent() {
            return Ok(());
        }
        let pool = self.pool()?;
        let rows = sqlx::query(
            "SELECT target_page_id, old_key FROM knowledge_link_rewrites WHERE due_at <= ?1",
        )
        .bind(now.to_wire())
        .fetch_all(pool)
        .await?;
        let mut by_target: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for r in &rows {
            by_target
                .entry(r.try_get("target_page_id")?)
                .or_default()
                .push(r.try_get("old_key")?);
        }
        for (target, keys) in by_target {
            let _guard = self.write_lock.lock().await;
            let id = KnowledgePageId::parse(&target).map_err(|_| store::corrupt())?;
            let mut conn = self.conn().await?;
            let row = store::page(&mut conn, id).await?;
            for k in &keys {
                sqlx::query("DELETE FROM knowledge_link_rewrites WHERE target_page_id = ?1 AND old_key = ?2")
                    .bind(&target)
                    .bind(k)
                    .execute(&mut *conn)
                    .await?;
            }
            drop(conn);
            if let Some(row) = row.filter(|r| r.state != KnowledgePageState::Removed) {
                self.rewrite_linking_pages_locked(&row, &keys).await;
            }
        }
        Ok(())
    }

    /// §Q step 2, run at once (a rename in ordin). The caller holds the write lock.
    pub(crate) async fn rewrite_linking_pages(&self, target: &PageRow, keys: &[String]) {
        self.rewrite_linking_pages_locked(target, keys).await;
    }

    async fn rewrite_linking_pages_locked(&self, target: &PageRow, keys: &[String]) {
        let linking: Vec<(KnowledgePageId, String)> = match self.linking_pages(target, keys).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    component = "knowledge",
                    code = e.code(),
                    "knowledge_link_rewrite_failed"
                );
                return;
            }
        };
        for (page, _) in linking {
            if let Err(e) = self.rewrite_one(page, target, keys).await {
                tracing::warn!(
                    component = "knowledge",
                    page_id = page.to_string().as_str(),
                    code = e.code(),
                    "knowledge_link_rewrite_failed"
                );
            }
        }
    }

    async fn linking_pages(
        &self,
        target: &PageRow,
        keys: &[String],
    ) -> Result<Vec<(KnowledgePageId, String)>, KnowledgeError> {
        let pool = self.pool()?;
        let rows = sqlx::query(
            "SELECT DISTINCT p.id, p.relative_path FROM knowledge_links l JOIN knowledge_pages p ON p.id = l.page_id \
             WHERE l.target_page_id = ?1 AND l.page_id <> ?1 AND p.state <> 'removed' \
             AND l.target_key IN (SELECT value FROM json_each(?2)) ORDER BY p.id",
        )
        .bind(target.id.to_string())
        .bind(store::to_json(&keys))
        .fetch_all(pool)
        .await?;
        rows.iter()
            .map(|r| {
                let id: String = r.try_get("id")?;
                Ok((
                    KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?,
                    r.try_get("relative_path")?,
                ))
            })
            .collect()
    }

    /// Rewrites one linking page's links to `target` (at most `REWRITE_ATTEMPTS` tries).
    async fn rewrite_one(
        &self,
        page: KnowledgePageId,
        target: &PageRow,
        keys: &[String],
    ) -> Result<(), KnowledgeError> {
        let key_set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        for _attempt in 0..REWRITE_ATTEMPTS {
            let mut conn = self.conn().await?;
            let Some(row) = store::page(&mut conn, page).await? else {
                return Ok(());
            };
            let pages = page_names(&mut conn, row.workspace_id).await?;
            drop(conn);
            let file = match self.files.read_file(row.place_id, &row.relative_path).await {
                Ok(f) => f,
                Err(e) => return Err(crate::pages::file_unavailable(&e)),
            };
            let text = String::from_utf8_lossy(&file.bytes).into_owned();
            let body_at = match okf::split_front_matter(&text) {
                Ok((_, body)) => text.len() - body.len(),
                Err(_) => 0,
            };
            let body = &text[body_at..];
            let stem_shared = pages.iter().filter(|p| p.id != target.id).any(|p| {
                name_key(&path_stem(&p.path)) == name_key(&path_stem(&target.relative_path))
            });
            let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
            for link in scan_links(body) {
                let Some(t) = link_target(&row.relative_path, &link) else {
                    continue;
                };
                if key_set.contains(name_key(&t).as_str()) {
                    edits.push((link.span.clone(), rewritten(&link, target, stem_shared)));
                }
            }
            if edits.is_empty() {
                return Ok(());
            }
            let mut new_text = text.clone();
            for (span, replacement) in edits.iter().rev() {
                new_text.replace_range(body_at + span.start..body_at + span.end, replacement);
            }
            let written = self
                .files
                .write_version(WriteVersion {
                    place_id: row.place_id,
                    path: row.relative_path.clone(),
                    bytes: new_text.clone().into_bytes(),
                    origin: FileVersionOrigin::Person,
                    expected_hash: Some(file.hash.clone()),
                })
                .await;
            match written {
                Ok(v) => {
                    let pool = self.pool()?;
                    let mut tx = pool.begin().await?;
                    let mut updated = row.clone();
                    updated.body = new_text[body_at..].to_owned();
                    updated.file_version = v.version;
                    updated.memory_indexed_version = None;
                    updated.memory_next_attempt_at = None;
                    store::fts_delete(&mut tx, &row).await?;
                    store::update_page(&mut tx, &updated).await?;
                    store::fts_insert(&mut tx, &updated).await?;
                    self.rebuild_links(&mut tx, &mut updated).await?;
                    tx.commit().await?;
                    self.journal(
                        "links_rewritten",
                        Some(row.workspace_id),
                        None,
                        "Links to a renamed knowledge page were updated.",
                        json!({ "page_id": page, "target_page_id": target.id, "links": edits.len() }),
                    )
                    .await;
                    self.wake_indexer();
                    return Ok(());
                }
                Err(FileError::ChangedMeanwhile { .. }) => {
                    // Re-read and re-run §P for L, then retry.
                    self.refresh_from_file(page).await?;
                    continue;
                }
                Err(e) => return Err(crate::pages::file_unavailable(&e)),
            }
        }
        tracing::warn!(
            component = "knowledge",
            page_id = page.to_string().as_str(),
            "knowledge_link_rewrite_failed"
        );
        let pool = self.pool()?;
        sqlx::query(
            "UPDATE knowledge_links SET target_page_id = NULL WHERE page_id = ?1 AND target_page_id = ?2 \
             AND target_key IN (SELECT value FROM json_each(?3))",
        )
        .bind(page.to_string())
        .bind(target.id.to_string())
        .bind(store::to_json(&keys))
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Reads a page's current file into its row and re-runs §P (a rewrite met a concurrent edit).
    async fn refresh_from_file(&self, page: KnowledgePageId) -> Result<(), KnowledgeError> {
        let mut conn = self.conn().await?;
        let Some(row) = store::page(&mut conn, page).await? else {
            return Ok(());
        };
        drop(conn);
        let Ok(file) = self.files.read_file(row.place_id, &row.relative_path).await else {
            return Ok(());
        };
        let version = match self
            .files
            .current_version(row.place_id, &row.relative_path)
            .await
        {
            Ok(Some(v)) => v.version,
            _ => row.file_version,
        };
        let text = String::from_utf8_lossy(&file.bytes).into_owned();
        let body = match okf::split_front_matter(&text) {
            Ok((_, b)) => b.to_owned(),
            Err(_) => text.clone(),
        };
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let mut updated = row.clone();
        updated.body = body;
        updated.file_version = version;
        store::fts_delete(&mut tx, &row).await?;
        store::update_page(&mut tx, &updated).await?;
        store::fts_insert(&mut tx, &updated).await?;
        self.rebuild_links(&mut tx, &mut updated).await?;
        tx.commit().await?;
        Ok(())
    }
}
