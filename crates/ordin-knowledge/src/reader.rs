//! Home and Search's reads (*Internal Logic* J, SA-RECON-03) and "Explained in" (ADR-111 §Y).

use std::sync::Arc;

use ordin_types::time::Timestamp;
use ordin_types::{KnowledgePageId, WorkspaceId};

use crate::error::KnowledgeError;
use crate::markdown::snippet;
use crate::model::{PageHit, PageNameMatch, PageSummary};
use crate::names::name_key;
use crate::ports::KnowledgeReader;
use crate::service::KnowledgeService;
use crate::store;

const SNIPPET_CHARS: usize = 300;

pub(crate) struct Reader {
    pub svc: Arc<KnowledgeService>,
}

#[async_trait::async_trait]
impl KnowledgeReader for Reader {
    /// J-2.
    async fn search_pages(
        &self,
        workspaces: &[WorkspaceId],
        query: &str,
        limit: u32,
    ) -> Result<Vec<PageHit>, KnowledgeError> {
        self.svc.desktop_only()?;
        let n = query.chars().count();
        if workspaces.is_empty()
            || workspaces.len() > 500
            || query.trim().is_empty()
            || n > 500
            || !(1..=50).contains(&limit)
        {
            tracing::warn!(
                component = "knowledge",
                code = "KNOW_QUERY_INVALID",
                "knowledge_reader_refused"
            );
            return Err(KnowledgeError::QueryInvalid);
        }
        let rows = self.svc.reader_search(workspaces, query, limit).await?;
        Ok(rows
            .into_iter()
            .map(|(p, rank)| PageHit {
                workspace_id: p.workspace_id,
                id: p.id,
                title: p.title.clone(),
                state: p.state,
                snippet: snippet(&p.body, query, SNIPPET_CHARS),
                rank,
            })
            .collect())
    }

    /// J-3.
    async fn out_of_date_since(
        &self,
        since: Timestamp,
    ) -> Result<Vec<PageSummary>, KnowledgeError> {
        self.svc.desktop_only()?;
        let pool = self.svc.pool()?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT p.id FROM knowledge_pages p JOIN knowledge_source_marks m ON m.page_id = p.id \
             WHERE p.state = 'may_be_out_of_date' AND m.left_at IS NULL AND m.marked_at >= ?1 \
             GROUP BY p.id ORDER BY MAX(m.marked_at) DESC, p.id LIMIT 200",
        )
        .bind(since.to_wire())
        .fetch_all(pool)
        .await?;
        let mut conn = self.svc.conn().await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let id = KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?;
            if let Some(row) = store::page(&mut conn, id).await? {
                out.push(self.svc.summary_of(&row));
            }
        }
        Ok(out)
    }

    /// J-4.
    async fn page_version(&self, id: KnowledgePageId) -> Result<Option<String>, KnowledgeError> {
        self.svc.desktop_only()?;
        let mut conn = self.svc.conn().await?;
        Ok(store::page(&mut conn, id)
            .await?
            .filter(|r| r.state != ordin_types::domain::places::KnowledgePageState::Removed)
            .map(|r| r.file_version.to_string()))
    }
}

impl KnowledgeService {
    /// ADR-111 §Y: the page whose title or other name matches one of `names` by name key.
    // spec: ordin-desktop/components/KnowledgeService#SA-KNOW-AMEM-02
    pub async fn page_for_names(
        &self,
        workspace_id: WorkspaceId,
        names: &[String],
    ) -> Result<Option<PageNameMatch>, KnowledgeError> {
        self.desktop_only()?;
        let keys: Vec<String> = names
            .iter()
            .filter(|n| (1..=512).contains(&n.chars().count()))
            .take(33)
            .map(|n| name_key(n))
            .collect();
        let pool = self.pool()?;
        let mut matched = None;
        for key in &keys {
            let own: Option<(String, String)> = sqlx::query_as(
                "SELECT p.id, p.title FROM knowledge_page_names n JOIN knowledge_pages p ON p.id = n.page_id \
                 WHERE n.workspace_id = ?1 AND n.name_key = ?2 AND p.state <> 'removed'",
            )
            .bind(workspace_id.to_string())
            .bind(key)
            .fetch_optional(pool)
            .await?;
            if let Some((id, title)) = own {
                matched = Some(PageNameMatch {
                    page_id: KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?,
                    title,
                });
                break;
            }
            let read: Option<(String, String)> = sqlx::query_as(
                "SELECT r.page_id, r.title FROM knowledge_read_page_names n JOIN knowledge_read_pages r \
                 ON r.workspace_id = n.workspace_id AND r.page_id = n.page_id \
                 WHERE n.workspace_id = ?1 AND n.name_key = ?2 ORDER BY r.last_changed_at DESC, r.page_id ASC LIMIT 1",
            )
            .bind(workspace_id.to_string())
            .bind(key)
            .fetch_optional(pool)
            .await?;
            if let Some((id, title)) = read {
                matched = Some(PageNameMatch {
                    page_id: crate::read::parse_page_id(&id)?,
                    title,
                });
                break;
            }
        }
        tracing::debug!(
            component = "knowledge",
            names = keys.len(),
            matched = matched.is_some(),
            "knowledge_page_for_names"
        );
        Ok(matched)
    }
}
