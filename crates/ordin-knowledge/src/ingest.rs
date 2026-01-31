//! The chat-history ingest step (*Internal Logic* I, SA-RECON-01; thread replies, ADR-107).

use std::sync::Arc;

use ordin_types::ScopeId;
use serde::{Deserialize, Serialize};

use crate::agent::{AgentState, Index};
use crate::error::KnowledgeError;
use crate::model::{ChatHistoryError, IngestStep, ItemKind};
use crate::ports::{KnowledgeIndex, KnowledgeIngest};

pub(crate) struct Ingest {
    pub index: Index,
    pub state: Arc<AgentState>,
}

/// I-3: the checkpoint; `threads` and `thread_cursor` default for a checkpoint written before
/// ADR-107, so a job under way carries on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    oldest: Option<String>,
    cursor: Option<String>,
    items_read: u64,
    #[serde(default)]
    threads: Vec<String>,
    #[serde(default)]
    thread_cursor: Option<String>,
}

fn map_history(e: ChatHistoryError) -> KnowledgeError {
    match e {
        ChatHistoryError::Unavailable => KnowledgeError::HistoryUnavailable,
        ChatHistoryError::Refused => KnowledgeError::ScopeNotEnabled,
    }
}

#[async_trait::async_trait]
impl KnowledgeIngest for Ingest {
    async fn ingest_step(
        &self,
        source: &str,
        scope: ScopeId,
        checkpoint: Option<&serde_json::Value>,
    ) -> Result<IngestStep, KnowledgeError> {
        // I-1
        let src = self
            .state
            .source(source)
            .ok_or(KnowledgeError::UnknownSource)?;
        let connection = match (src.kind, &src.connection) {
            (ItemKind::ChatHistory, Some(c)) => c.clone(),
            _ => return Err(KnowledgeError::UnknownKind),
        };
        // I-3 (the shape is checked before anything is read).
        let given = match checkpoint {
            Some(v) => Some(
                serde_json::from_value::<Checkpoint>(v.clone())
                    .map_err(|_| KnowledgeError::CheckpointInvalid)?,
            ),
            None => None,
        };
        // I-2
        let chat = match self.state.chat_history.chat_of(scope).await {
            Ok(Some(c)) if c.ingest_enabled && c.connection == connection => c,
            Ok(_) => return Err(KnowledgeError::ScopeNotEnabled),
            Err(e) => return Err(map_history(e)),
        };
        let mut cp = match given {
            Some(c) => c,
            None => {
                let newest = self.index.recent(source, Some(scope), 1).await?;
                Checkpoint {
                    oldest: newest.first().and_then(|i| i.origin_id.clone()),
                    ..Checkpoint::default()
                }
            }
        };
        let metrics = &self.state.metrics;
        // I-4: a thread step reads replies and no channel page.
        if let Some(thread) = cp.threads.first().cloned() {
            match self
                .state
                .chat_history
                .replies(
                    &connection,
                    &chat.chat,
                    &thread,
                    cp.thread_cursor.as_deref(),
                )
                .await
            {
                Err(_) => {
                    metrics.ingest_thread_failed(source);
                    tracing::warn!(component = "knowledge", source, scope_id = %scope, "knowledge_ingest_thread_failed");
                    cp.threads.remove(0);
                    cp.thread_cursor = None;
                }
                Ok(page) => {
                    let n = page.items.len() as u64;
                    if let Err(e) = self.index.upsert(source, scope, &page.items).await {
                        metrics.ingest_thread_failed(source);
                        return Err(e);
                    }
                    if n > 0 {
                        metrics.items_written(source, n);
                    }
                    cp.items_read += n;
                    cp.thread_cursor = page.next_cursor;
                    if cp.thread_cursor.is_none() {
                        cp.threads.remove(0);
                    }
                    tracing::info!(component = "knowledge", source, scope_id = %scope, items = n, done = false, "knowledge_ingest_step");
                    return Ok(IngestStep {
                        items_read: cp.items_read,
                        checkpoint: serde_json::to_value(&cp).unwrap_or_default(),
                        total: None,
                        done: false,
                    });
                }
            }
            tracing::info!(component = "knowledge", source, scope_id = %scope, items = 0, done = false, "knowledge_ingest_step");
            return Ok(IngestStep {
                items_read: cp.items_read,
                checkpoint: serde_json::to_value(&cp).unwrap_or_default(),
                total: None,
                done: false,
            });
        }
        // I-5: a page step.
        let page = match self
            .state
            .chat_history
            .page(
                &connection,
                &chat.chat,
                cp.oldest.as_deref(),
                cp.cursor.as_deref(),
            )
            .await
        {
            Ok(p) => p,
            Err(e) => {
                metrics.ingest_thread_failed(source);
                return Err(map_history(e));
            }
        };
        // I-6
        let n = page.items.len() as u64;
        if let Err(e) = self.index.upsert(source, scope, &page.items).await {
            metrics.ingest_thread_failed(source);
            return Err(e);
        }
        if n > 0 {
            metrics.items_written(source, n);
        }
        let done = page.next_cursor.is_none() && page.threads.is_empty();
        let next = Checkpoint {
            oldest: cp.oldest.clone(),
            cursor: page.next_cursor,
            items_read: cp.items_read + n,
            threads: page.threads,
            thread_cursor: None,
        };
        tracing::info!(component = "knowledge", source, scope_id = %scope, items = n, done, "knowledge_ingest_step");
        Ok(IngestStep {
            items_read: next.items_read,
            checkpoint: serde_json::to_value(&next).unwrap_or_default(),
            total: None,
            done,
        })
    }
}
