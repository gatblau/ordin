//! The memory-index task (*Internal Logic* F, with the read pages of ADR-110, the page graph of
//! ADR-111 and the standing of ADR-112) and the workspace index `knowledge/index.md` (AD).

use std::sync::Arc;

use ordin_files::{FileError, FileVersionOrigin, WriteVersion};
use ordin_memory_service::{PageGraph, PageSection, PageStanding};
use ordin_types::time::Timestamp;
use ordin_types::{KnowledgePageId, TeamFolderId, WorkspaceId};
use sqlx::Row;
use tokio_util::sync::CancellationToken;

use crate::error::KnowledgeError;
use crate::markdown::split_sections;
use crate::names::below_knowledge;
use crate::okf::{self, IndexEntry};
use crate::service::{lock, KnowledgeService, INDEX_IDLE};
use crate::store::{self, PageRow};

const BATCH: i64 = 20;
const OTHER_NAMES_MAX: usize = 8;
const LINKS_TO_MAX: usize = 15;

/// `min(30 s × 2^(attempts−1), 30 min)`.
pub(crate) fn backoff(attempts: u32) -> chrono::Duration {
    let secs = 30i64.saturating_mul(1i64 << attempts.saturating_sub(1).min(20));
    chrono::Duration::seconds(secs.min(30 * 60))
}

fn sections_of(title: &str, body: &str) -> Vec<PageSection> {
    split_sections(title, body)
        .into_iter()
        .map(|(heading, text)| PageSection { heading, text })
        .collect()
}

impl KnowledgeService {
    pub(crate) async fn index_task(self: Arc<Self>, shutdown: CancellationToken) {
        loop {
            if let Err(e) = self.index_pass().await {
                tracing::warn!(
                    component = "knowledge",
                    code = e.code(),
                    "knowledge_index_pass_failed"
                );
            }
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = self.index_wake.notified() => {}
                _ = tokio::time::sleep(INDEX_IDLE) => {}
            }
        }
    }

    /// The page graph of an own page (ADR-111 F step 2, ADR-112 F).
    pub(crate) async fn page_graph(&self, row: &PageRow) -> Result<PageGraph, KnowledgeError> {
        let pool = self.pool()?;
        let links = sqlx::query(
            "SELECT l.target_page_id, p.title FROM knowledge_links l JOIN knowledge_pages p ON p.id = l.target_page_id \
             WHERE l.page_id = ?1 AND l.target_page_id IS NOT NULL AND l.target_page_id <> ?1 AND p.state <> 'removed' \
             ORDER BY l.ordinal",
        )
        .bind(row.id.to_string())
        .fetch_all(pool)
        .await?;
        let mut links_to: Vec<(KnowledgePageId, String)> = Vec::new();
        for l in &links {
            let id: String = l.try_get("target_page_id")?;
            let id = KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?;
            if !links_to.iter().any(|(i, _)| *i == id) {
                links_to.push((id, l.try_get("title")?));
            }
        }
        if links_to.len() > LINKS_TO_MAX {
            let left_out = links_to.len() - LINKS_TO_MAX;
            tracing::info!(component = "knowledge", page_id = %row.id, left_out, "knowledge_page_links_capped");
            links_to.truncate(LINKS_TO_MAX);
        }
        Ok(PageGraph {
            title: row.title.clone(),
            other_names: row.aliases.iter().take(OTHER_NAMES_MAX).cloned().collect(),
            links_to,
            standing: PageStanding {
                tier: row.okf_tier,
                status: row.okf_status,
                past_stale_after: row
                    .stale_after
                    .as_deref()
                    .and_then(okf::parse_stale_after)
                    .is_some_and(|s| s <= Timestamp::now()),
            },
        })
    }

    /// One pass of F: up to 20 own pages, up to 20 read pages, the removals and the tombstones.
    pub(crate) async fn index_pass(&self) -> Result<(), KnowledgeError> {
        if self.program.is_agent() {
            return Ok(());
        }
        let pool = self.pool()?;
        let now = Timestamp::now();
        self.stale_pass(now).await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM knowledge_pages WHERE state <> 'removed' \
             AND (memory_indexed_version IS NULL OR memory_indexed_version < file_version) \
             AND (memory_next_attempt_at IS NULL OR memory_next_attempt_at <= ?1) \
             ORDER BY last_changed_at LIMIT ?2",
        )
        .bind(now.to_wire())
        .bind(BATCH)
        .fetch_all(pool)
        .await?;
        for id in ids {
            let id = KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?;
            let mut conn = self.conn().await?;
            let Some(row) = store::page(&mut conn, id).await? else {
                continue;
            };
            drop(conn);
            let graph = self.page_graph(&row).await?;
            let result = self
                .memory
                .index_page_sections(
                    row.workspace_id,
                    row.id,
                    sections_of(&row.title, &row.body),
                    row.file_version.to_string(),
                    graph,
                )
                .await;
            self.record_attempt(
                "knowledge_pages",
                "id",
                &row.id.to_string(),
                row.memory_index_attempts,
                row.file_version,
                result.map_err(|e| e.code()),
            )
            .await?;
        }
        // Removed pages whose sections are still in memory.
        let removed: Vec<(String, String)> =
            sqlx::query_as("SELECT id, workspace_id FROM knowledge_pages WHERE state = 'removed' AND memory_removed = 0 LIMIT ?1")
                .bind(BATCH)
                .fetch_all(pool)
                .await?;
        for (id, workspace) in removed {
            let pid = KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?;
            let workspace = WorkspaceId::parse(&workspace).map_err(|_| store::corrupt())?;
            // spec: ordin-desktop/components/KnowledgeService#KNOW-REC-1
            if self.memory.remove_page(workspace, None, pid).await.is_ok() {
                sqlx::query("UPDATE knowledge_pages SET memory_removed = 1 WHERE id = ?1")
                    .bind(&id)
                    .execute(pool)
                    .await?;
            }
        }
        self.read_index_pass(now).await?;
        Ok(())
    }

    /// Success → indexed version and no attempts; error → back-off (F step 2).
    pub(crate) async fn record_attempt(
        &self,
        table: &'static str,
        _key: &'static str,
        id: &str,
        attempts: u32,
        version: u32,
        result: Result<(), &'static str>,
    ) -> Result<(), KnowledgeError> {
        let pool = self.pool()?;
        match result {
            Ok(()) => {
                let sql = match table {
                    "knowledge_pages" => "UPDATE knowledge_pages SET memory_indexed_version = ?2, memory_index_attempts = 0, memory_next_attempt_at = NULL WHERE id = ?1",
                    _ => "UPDATE knowledge_read_pages SET memory_indexed_version = ?2, memory_index_attempts = 0, memory_next_attempt_at = NULL WHERE page_id = ?1",
                };
                sqlx::query(sql)
                    .bind(id)
                    .bind(i64::from(version))
                    .execute(pool)
                    .await?;
            }
            Err(code) => {
                let attempts = attempts + 1;
                let next = Timestamp(Timestamp::now().0 + backoff(attempts));
                let sql = match table {
                    "knowledge_pages" => "UPDATE knowledge_pages SET memory_index_attempts = ?2, memory_next_attempt_at = ?3 WHERE id = ?1",
                    _ => "UPDATE knowledge_read_pages SET memory_index_attempts = ?2, memory_next_attempt_at = ?3 WHERE page_id = ?1",
                };
                sqlx::query(sql)
                    .bind(id)
                    .bind(i64::from(attempts))
                    .bind(next.to_wire())
                    .execute(pool)
                    .await?;
                tracing::warn!(
                    component = "knowledge",
                    page_id = id,
                    attempts,
                    code,
                    "knowledge_memory_index_failed"
                );
            }
        }
        Ok(())
    }

    /// ADR-112 F: once an hour, pages whose `stale_after` lies between the last run and now are
    /// re-indexed once.
    async fn stale_pass(&self, now: Timestamp) -> Result<(), KnowledgeError> {
        let last = *lock(&self.stale_checked_at);
        if last.is_some_and(|l| (now.0 - l.0) < chrono::Duration::hours(1)) {
            return Ok(());
        }
        *lock(&self.stale_checked_at) = Some(now);
        let Some(last) = last else {
            return Ok(());
        };
        let pool = self.pool()?;
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, stale_after FROM knowledge_pages WHERE state <> 'removed' AND stale_after IS NOT NULL",
        )
        .fetch_all(pool)
        .await?;
        for (id, raw) in rows {
            if okf::parse_stale_after(&raw).is_some_and(|s| s > last && s <= now) {
                sqlx::query(
                    "UPDATE knowledge_pages SET memory_indexed_version = NULL WHERE id = ?1",
                )
                .bind(&id)
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    /// F (ADR-110 addition, ADR-111 read-page graph): read pages and tombstones.
    async fn read_index_pass(&self, now: Timestamp) -> Result<(), KnowledgeError> {
        let pool = self.pool()?;
        let rows = sqlx::query(
            "SELECT workspace_id, page_id FROM knowledge_read_pages \
             WHERE (memory_indexed_version IS NULL OR memory_indexed_version < version) \
             AND (memory_next_attempt_at IS NULL OR memory_next_attempt_at <= ?1) ORDER BY got_at LIMIT ?2",
        )
        .bind(now.to_wire())
        .bind(BATCH)
        .fetch_all(pool)
        .await?;
        for r in &rows {
            let w: String = r.try_get("workspace_id")?;
            let p: String = r.try_get("page_id")?;
            let workspace = WorkspaceId::parse(&w).map_err(|_| store::corrupt())?;
            let page = crate::read::parse_page_id(&p)?;
            let mut conn = self.conn().await?;
            let Some(read) = crate::read::read_row(&mut conn, workspace, page).await? else {
                continue;
            };
            let links = sqlx::query(
                "SELECT l.target_page_id, t.title FROM knowledge_read_links l JOIN knowledge_read_pages t \
                 ON t.workspace_id = l.workspace_id AND t.page_id = l.target_page_id \
                 WHERE l.workspace_id = ?1 AND l.page_id = ?2 AND l.target_page_id IS NOT NULL AND l.target_page_id <> ?2 \
                 AND t.folder_id = ?3 ORDER BY l.ordinal",
            )
            .bind(&w)
            .bind(&p)
            .bind(read.folder_id.to_string())
            .fetch_all(&mut *conn)
            .await?;
            drop(conn);
            let mut links_to: Vec<(KnowledgePageId, String)> = Vec::new();
            for l in &links {
                let id: String = l.try_get("target_page_id")?;
                let id = crate::read::parse_page_id(&id)?;
                if !links_to.iter().any(|(i, _)| *i == id) {
                    links_to.push((id, l.try_get("title")?));
                }
            }
            if links_to.len() > LINKS_TO_MAX {
                let left_out = links_to.len() - LINKS_TO_MAX;
                tracing::info!(component = "knowledge", page_id = %page, left_out, "knowledge_page_links_capped");
                links_to.truncate(LINKS_TO_MAX);
            }
            let graph = PageGraph {
                title: read.title.clone(),
                other_names: read.aliases.iter().take(OTHER_NAMES_MAX).cloned().collect(),
                links_to,
                standing: PageStanding {
                    tier: read.okf_tier,
                    status: read.okf_status,
                    past_stale_after: read
                        .stale_after
                        .as_deref()
                        .and_then(okf::parse_stale_after)
                        .is_some_and(|s| s <= now),
                },
            };
            let result = self
                .memory
                .index_read_page_sections(
                    workspace,
                    read.folder_id,
                    page,
                    sections_of(&read.title, &read.body),
                    read.version.to_string(),
                    graph,
                )
                .await;
            self.record_attempt(
                "knowledge_read_pages",
                "page_id",
                &p,
                read.memory_index_attempts,
                read.version,
                result.map_err(|e| e.code()),
            )
            .await?;
        }
        let tombstones: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT page_id, workspace_id, folder_id FROM knowledge_read_pages_removed WHERE memory_removed = 0 LIMIT ?1",
        )
        .bind(BATCH)
        .fetch_all(pool)
        .await?;
        for (p, workspace, folder) in tombstones {
            let page = crate::read::parse_page_id(&p)?;
            let workspace = WorkspaceId::parse(&workspace).map_err(|_| store::corrupt())?;
            let folder =
                TeamFolderId(uuid::Uuid::parse_str(&folder).map_err(|_| store::corrupt())?);
            // spec: ordin-desktop/components/KnowledgeService#KNOW-REC-1
            if self
                .memory
                .remove_page(workspace, Some(folder), page)
                .await
                .is_ok()
            {
                sqlx::query("DELETE FROM knowledge_read_pages_removed WHERE page_id = ?1")
                    .bind(&p)
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// AD-2 to AD-4: writes `knowledge/index.md` when it is ordin's and its bytes changed.
    pub(crate) async fn write_workspace_index(&self, workspace: WorkspaceId) {
        if let Err(e) = self.write_index_inner(workspace).await {
            tracing::warn!(component = "knowledge", workspace_id = %workspace, code = e.code(), "knowledge_index_failed");
        }
    }

    async fn write_index_inner(&self, workspace: WorkspaceId) -> Result<(), KnowledgeError> {
        let ws = self.workspace(workspace).await?;
        let mut conn = self.conn().await?;
        let rows = store::workspace_pages(&mut conn, workspace).await?;
        drop(conn);
        if rows.is_empty() {
            return Ok(());
        }
        let path = "knowledge/index.md";
        let (existing, hash) = match self.files.read_file(ws.own_folder_place_id, path).await {
            Ok(f) => (
                Some(String::from_utf8_lossy(&f.bytes).into_owned()),
                Some(f.hash),
            ),
            Err(FileError::NotFound) => (None, None),
            Err(e) => {
                tracing::warn!(component = "knowledge", workspace_id = %workspace, code = e.code(), "knowledge_index_failed");
                return Ok(());
            }
        };
        if let Some(text) = &existing {
            let ours = okf::split_front_matter(text)
                .ok()
                .and_then(|(h, _)| h)
                .and_then(|h| ordin_types::yaml::read_restricted_node(h, okf::HEADER_MAX_BYTES).ok())
                .and_then(|n| match n {
                    ordin_types::yaml::YamlNode::Map(m) => m
                        .into_iter()
                        .find(|(k, _)| k == "generated")
                        .and_then(|(_, g)| match g {
                            ordin_types::yaml::YamlNode::Map(g) => g.into_iter().find(|(k, _)| k == "by").map(|(_, v)| v),
                            _ => None,
                        }),
                    _ => None,
                })
                .is_some_and(|by| matches!(by, ordin_types::yaml::YamlNode::Scalar { ref text, .. } if text.starts_with("ordin/")));
            if !ours {
                let version = self
                    .files
                    .current_version(ws.own_folder_place_id, path)
                    .await
                    .ok()
                    .flatten()
                    .map(|v| v.version)
                    .unwrap_or(0);
                if lock(&self.index_not_ours).insert((workspace, version)) {
                    tracing::info!(component = "knowledge", workspace_id = %workspace, "knowledge_index_not_ours");
                }
                return Ok(());
            }
        }
        let entries: Vec<IndexEntry> = rows
            .iter()
            .map(|r| IndexEntry {
                path: below_knowledge(&r.relative_path),
                title: r.title.clone(),
                description: r.description.clone(),
            })
            .collect();
        let text = okf::render_index(
            &entries,
            &format!("ordin/{}", self.app_version),
            Timestamp::now(),
        );
        if let Some(old) = &existing {
            if without_generated_at(old) == without_generated_at(&text) {
                return Ok(());
            }
        }
        match self
            .files
            .write_version(WriteVersion {
                place_id: ws.own_folder_place_id,
                path: path.to_owned(),
                bytes: text.into_bytes(),
                origin: FileVersionOrigin::Person,
                expected_hash: hash,
            })
            .await
        {
            Ok(_) => {
                tracing::info!(component = "knowledge", workspace_id = %workspace, pages = rows.len(), "knowledge_index_written");
            }
            Err(FileError::ChangedMeanwhile { .. }) => self.schedule_index(workspace),
            Err(e) => {
                tracing::warn!(component = "knowledge", workspace_id = %workspace, code = e.code(), "knowledge_index_failed");
            }
        }
        Ok(())
    }
}

fn without_generated_at(text: &str) -> String {
    text.lines()
        .filter(|l| !l.starts_with("  at: "))
        .collect::<Vec<_>>()
        .join("\n")
}
