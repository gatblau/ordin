//! Search and filters: the person's filter words (§S step 1), the FTS candidates of the
//! `knowledge_search_pages` tool (G-3, S-2, V-1) and of Home and Search (J-2), with the
//! deprecated rule of ADR-112 (G and V changes).

use ordin_types::domain::places::{KnowledgeAuthor, KnowledgePageState};
use ordin_types::wire::knowledge_query::PageStatus;
use ordin_types::{KnowledgePageId, WorkspaceId};
use sqlx::Row;

use crate::error::KnowledgeError;
use crate::markdown::fts_any_word;
use crate::names::name_key;
use crate::read::ReadRow;
use crate::service::KnowledgeService;
use crate::store::{self, PageRow};

/// The words of `list_pages`'s filter (§S step 1).
#[derive(Debug, Default)]
pub(crate) struct ListFilter {
    tags: Vec<String>,
    state: Option<KnowledgePageState>,
    written_by: Option<KnowledgeAuthor>,
    words: Vec<String>,
}

impl ListFilter {
    pub(crate) fn parse(raw: &str) -> ListFilter {
        let mut f = ListFilter::default();
        for w in raw.split_whitespace() {
            if let Some(t) = w.strip_prefix("tag:").filter(|t| !t.is_empty()) {
                f.tags.push(name_key(t.trim_start_matches('#')));
            } else if w == "state:out-of-date" {
                f.state = Some(KnowledgePageState::MayBeOutOfDate);
            } else if w == "state:current" {
                f.state = Some(KnowledgePageState::Current);
            } else if w == "written-by:ordin" {
                f.written_by = Some(KnowledgeAuthor::Ordin);
            } else if w == "written-by:person" {
                f.written_by = Some(KnowledgeAuthor::Person);
            } else {
                f.words.push(w.to_lowercase());
            }
        }
        f
    }

    pub(crate) fn keeps(&self, row: &PageRow) -> bool {
        let tags: Vec<String> = row.tags.iter().map(|t| name_key(t)).collect();
        if !self.tags.iter().all(|t| tags.contains(t)) {
            return false;
        }
        if self.state.is_some_and(|s| s != row.state) {
            return false;
        }
        if self.written_by.is_some_and(|w| w != row.written_by) {
            return false;
        }
        let names: Vec<String> = std::iter::once(&row.title)
            .chain(row.aliases.iter())
            .map(|n| n.to_lowercase())
            .collect();
        self.words
            .iter()
            .all(|w| names.iter().any(|n| n.contains(w.as_str())))
    }
}

/// The tool's filters (S-2).
#[derive(Debug, Clone, Default)]
pub(crate) struct ToolFilter {
    pub query: Option<String>,
    pub state: Option<KnowledgePageState>,
    pub written_by: Option<KnowledgeAuthor>,
    pub tag: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum Candidate {
    Own(PageRow),
    Read(ReadRow),
}

impl Candidate {
    pub(crate) fn status(&self) -> PageStatus {
        match self {
            Candidate::Own(r) => r.okf_status,
            Candidate::Read(r) => r.okf_status,
        }
    }
}

/// ADR-112: deprecated candidates are left out while one that is not deprecated remains.
pub(crate) fn drop_deprecated<T>(items: &mut Vec<T>, status: impl Fn(&T) -> PageStatus) {
    if items.iter().any(|i| status(i) != PageStatus::Deprecated) {
        items.retain(|i| status(i) != PageStatus::Deprecated);
    }
}

fn state_word(s: KnowledgePageState) -> &'static str {
    store::state_text(s)
}

impl KnowledgeService {
    async fn own_candidates(
        &self,
        workspace: WorkspaceId,
        f: &ToolFilter,
        limit: i64,
    ) -> Result<Vec<PageRow>, KnowledgeError> {
        let pool = self.pool()?;
        let state = f.state.map(state_word);
        let by = f.written_by.map(store::author_text);
        let tag = f
            .tag
            .as_deref()
            .map(|t| name_key(t.trim_start_matches('#')));
        let ids: Vec<String> = match f.query.as_deref() {
            Some(q) => {
                let Some(m) = fts_any_word(q) else {
                    return Ok(Vec::new());
                };
                sqlx::query_scalar(
                    "SELECT p.id FROM knowledge_pages_fts JOIN knowledge_pages p ON p.rowid = knowledge_pages_fts.rowid \
                     WHERE knowledge_pages_fts MATCH ?1 AND p.workspace_id = ?2 AND p.state <> 'removed' \
                     AND (?3 IS NULL OR p.state = ?3) AND (?4 IS NULL OR p.written_by = ?4) \
                     AND (?5 IS NULL OR EXISTS (SELECT 1 FROM knowledge_page_tags t WHERE t.page_id = p.id AND t.tag_key = ?5)) \
                     ORDER BY bm25(knowledge_pages_fts, 2.0, 1.0), p.id LIMIT ?6",
                )
                .bind(m)
                .bind(workspace.to_string())
                .bind(state)
                .bind(by)
                .bind(tag)
                .bind(limit)
                .fetch_all(pool)
                .await?
            }
            None => {
                sqlx::query_scalar(
                    "SELECT p.id FROM knowledge_pages p WHERE p.workspace_id = ?1 AND p.state <> 'removed' \
                     AND (?2 IS NULL OR p.state = ?2) AND (?3 IS NULL OR p.written_by = ?3) \
                     AND (?4 IS NULL OR EXISTS (SELECT 1 FROM knowledge_page_tags t WHERE t.page_id = p.id AND t.tag_key = ?4)) \
                     ORDER BY p.last_changed_at DESC, p.id LIMIT ?5",
                )
                .bind(workspace.to_string())
                .bind(state)
                .bind(by)
                .bind(tag)
                .bind(limit)
                .fetch_all(pool)
                .await?
            }
        };
        let mut conn = self.conn().await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let id = KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?;
            if let Some(r) = store::page(&mut conn, id).await? {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// The candidates of `knowledge_search_pages` (G-3, S-2, V-1): own and read pages merged by
    /// rank position, own first at equal position, up to `limit × 3`, deprecated rule applied.
    pub(crate) async fn tool_candidates(
        &self,
        workspace: WorkspaceId,
        f: &ToolFilter,
        limit: u32,
    ) -> Result<Vec<Candidate>, KnowledgeError> {
        let wanted = i64::from(limit) * 3;
        let own = self.own_candidates(workspace, f, wanted).await?;
        let read = self.read_candidates(workspace, f, wanted).await?;
        let mut merged = Vec::with_capacity(own.len() + read.len());
        let mut own = own.into_iter();
        let mut read = read.into_iter();
        loop {
            let a = own.next();
            let b = read.next();
            if a.is_none() && b.is_none() {
                break;
            }
            if let Some(a) = a {
                merged.push(Candidate::Own(a));
            }
            if let Some(b) = b {
                merged.push(Candidate::Read(b));
            }
        }
        merged.truncate(usize::try_from(wanted).unwrap_or(usize::MAX));
        drop_deprecated(&mut merged, Candidate::status);
        Ok(merged)
    }

    /// J-2: own pages of the listed workspaces, best first.
    pub(crate) async fn reader_search(
        &self,
        workspaces: &[WorkspaceId],
        query: &str,
        limit: u32,
    ) -> Result<Vec<(PageRow, f64)>, KnowledgeError> {
        let Some(m) = fts_any_word(query) else {
            return Ok(Vec::new());
        };
        let list: Vec<String> = workspaces.iter().map(ToString::to_string).collect();
        let pool = self.pool()?;
        let rows = sqlx::query(
            "SELECT p.id, bm25(knowledge_pages_fts, 2.0, 1.0) AS rank FROM knowledge_pages_fts \
             JOIN knowledge_pages p ON p.rowid = knowledge_pages_fts.rowid \
             WHERE knowledge_pages_fts MATCH ?1 AND p.state <> 'removed' \
             AND p.workspace_id IN (SELECT value FROM json_each(?2)) \
             ORDER BY rank, p.id LIMIT ?3",
        )
        .bind(m)
        .bind(store::to_json(&list))
        .bind(i64::from(limit) * 3)
        .fetch_all(pool)
        .await?;
        let mut conn = self.conn().await?;
        let mut out = Vec::new();
        for r in &rows {
            let id: String = r.try_get("id")?;
            let rank: f64 = r.try_get("rank")?;
            if let Some(p) = store::page(
                &mut conn,
                KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?,
            )
            .await?
            {
                out.push((p, -rank));
            }
        }
        drop_deprecated(&mut out, |(p, _)| p.okf_status);
        out.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(out)
    }
}
