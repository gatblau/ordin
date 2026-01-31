//! K1–K19 on SQLite with FTS5 (the single shape, H-8): two external-content FTS tables, the
//! Porter one for `english` and the plain one for every other language, kept in step by triggers.
//! The any-word query is built from double-quoted tokens, so query syntax in the input is inert.

use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{RunId, ScopeId};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::agent::{has_link, has_steps, sha256_hex, ItemStore, Prepared, Ranked, Vis};
use crate::error::KnowledgeError;
use crate::markdown::fts_any_word;
use crate::model::{ItemKind, KnowledgeItem, Sender};

pub(crate) struct SqliteItems {
    pub pool: SqlitePool,
}

fn wire(t: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::SubsecRound;
    Timestamp(t.trunc_subsecs(3)).to_wire()
}

fn time(raw: &str) -> Result<chrono::DateTime<chrono::Utc>, KnowledgeError> {
    parse_rfc3339_ms_utc(raw).map_err(|_| crate::store::corrupt())
}

fn item_from(r: &SqliteRow) -> Result<KnowledgeItem, KnowledgeError> {
    let id: String = r.try_get("item_id")?;
    let scope: Option<String> = r.try_get("scope_id")?;
    let run: Option<String> = r.try_get("run_id")?;
    let kind: String = r.try_get("kind")?;
    Ok(KnowledgeItem {
        item_id: uuid::Uuid::parse_str(&id).map_err(|_| crate::store::corrupt())?,
        source: r.try_get("source")?,
        kind: ItemKind::from_text(&kind).ok_or_else(crate::store::corrupt)?,
        scope_id: scope
            .map(|s| ScopeId::parse(&s).map_err(|_| crate::store::corrupt()))
            .transpose()?,
        origin_id: r.try_get("origin_id")?,
        origin_time: time(&r.try_get::<String, _>("origin_time")?)?,
        sender: r.try_get("sender")?,
        sender_name: r.try_get("sender_name")?,
        from_program: r.try_get::<i64, _>("from_program")? != 0,
        own: r.try_get::<i64, _>("own")? != 0,
        text: r.try_get("body")?,
        thread: r.try_get("thread_root")?,
        verified: r.try_get::<Option<i64>, _>("verified")?.map(|v| v != 0),
        run_id: run
            .map(|s| RunId::parse(&s).map_err(|_| crate::store::corrupt()))
            .transpose()?,
        retired: r.try_get::<i64, _>("retired")? != 0,
    })
}

fn scope_text(s: Option<ScopeId>) -> Option<String> {
    s.map(|s| s.to_string())
}

fn hidden_json(v: &Vis) -> String {
    serde_json::to_string(&v.hidden).unwrap_or_else(|_| "[]".to_owned())
}

// The visibility predicate V: ?2 scope, ?3 include_own, ?4 hidden senders (JSON array).
const SEARCH_PORTER: &str = "SELECT i.item_id, i.source, i.kind, i.scope_id, i.origin_id, i.origin_time, i.sender, i.sender_name, \
    i.from_program, i.own, i.body, i.body_hash, i.thread_root, i.verified, i.run_id, i.retired, \
    -bm25(knowledge_items_fts_porter) * CASE WHEN ?5 AND (i.has_link OR i.has_steps) THEN 1.5 ELSE 1.0 END AS rank \
    FROM knowledge_items_fts_porter JOIN knowledge_items i ON i.rowid = knowledge_items_fts_porter.rowid \
    WHERE knowledge_items_fts_porter MATCH ?6 AND i.source = ?1 \
    AND (i.scope_id IS ?2 OR (i.kind = 'notes' AND i.scope_id IS NULL)) AND i.retired = 0 \
    AND (i.kind <> 'entries' OR i.verified = 1) \
    AND (i.kind <> 'chat-history' OR ((?3 OR i.own = 0) AND i.sender NOT IN (SELECT value FROM json_each(?4)))) \
    ORDER BY rank DESC, i.origin_time DESC, i.item_id LIMIT ?7";
const SEARCH_PLAIN: &str = "SELECT i.item_id, i.source, i.kind, i.scope_id, i.origin_id, i.origin_time, i.sender, i.sender_name, \
    i.from_program, i.own, i.body, i.body_hash, i.thread_root, i.verified, i.run_id, i.retired, \
    -bm25(knowledge_items_fts_plain) * CASE WHEN ?5 AND (i.has_link OR i.has_steps) THEN 1.5 ELSE 1.0 END AS rank \
    FROM knowledge_items_fts_plain JOIN knowledge_items i ON i.rowid = knowledge_items_fts_plain.rowid \
    WHERE knowledge_items_fts_plain MATCH ?6 AND i.source = ?1 \
    AND (i.scope_id IS ?2 OR (i.kind = 'notes' AND i.scope_id IS NULL)) AND i.retired = 0 \
    AND (i.kind <> 'entries' OR i.verified = 1) \
    AND (i.kind <> 'chat-history' OR ((?3 OR i.own = 0) AND i.sender NOT IN (SELECT value FROM json_each(?4)))) \
    ORDER BY rank DESC, i.origin_time DESC, i.item_id LIMIT ?7";
const REPLIES_PORTER: &str = "SELECT i.item_id, i.source, i.kind, i.scope_id, i.origin_id, i.origin_time, i.sender, i.sender_name, \
    i.from_program, i.own, i.body, i.body_hash, i.thread_root, i.verified, i.run_id, i.retired, \
    -bm25(knowledge_items_fts_porter) AS rank \
    FROM knowledge_items_fts_porter JOIN knowledge_items i ON i.rowid = knowledge_items_fts_porter.rowid \
    WHERE knowledge_items_fts_porter MATCH ?5 AND i.source = ?1 \
    AND i.thread_root IN (SELECT value FROM json_each(?6)) AND i.origin_id <> i.thread_root \
    AND (i.scope_id IS ?2 OR (i.kind = 'notes' AND i.scope_id IS NULL)) AND i.retired = 0 \
    AND (i.kind <> 'entries' OR i.verified = 1) \
    AND (i.kind <> 'chat-history' OR ((?3 OR i.own = 0) AND i.sender NOT IN (SELECT value FROM json_each(?4)))) \
    ORDER BY i.thread_root, rank DESC, i.origin_time DESC, i.item_id";
const REPLIES_PLAIN: &str = "SELECT i.item_id, i.source, i.kind, i.scope_id, i.origin_id, i.origin_time, i.sender, i.sender_name, \
    i.from_program, i.own, i.body, i.body_hash, i.thread_root, i.verified, i.run_id, i.retired, \
    -bm25(knowledge_items_fts_plain) AS rank \
    FROM knowledge_items_fts_plain JOIN knowledge_items i ON i.rowid = knowledge_items_fts_plain.rowid \
    WHERE knowledge_items_fts_plain MATCH ?5 AND i.source = ?1 \
    AND i.thread_root IN (SELECT value FROM json_each(?6)) AND i.origin_id <> i.thread_root \
    AND (i.scope_id IS ?2 OR (i.kind = 'notes' AND i.scope_id IS NULL)) AND i.retired = 0 \
    AND (i.kind <> 'entries' OR i.verified = 1) \
    AND (i.kind <> 'chat-history' OR ((?3 OR i.own = 0) AND i.sender NOT IN (SELECT value FROM json_each(?4)))) \
    ORDER BY i.thread_root, rank DESC, i.origin_time DESC, i.item_id";

#[async_trait::async_trait]
impl ItemStore for SqliteItems {
    /// K1: one statement for the call, from one bound JSON array.
    async fn upsert(
        &self,
        source: &str,
        scope: ScopeId,
        connection: &str,
        language: &str,
        items: &[Prepared],
    ) -> Result<u64, KnowledgeError> {
        let now = Timestamp::now().to_wire();
        let rows: Vec<serde_json::Value> = items
            .iter()
            .map(|i| {
                serde_json::json!({
                    "item_id": i.item_id.to_string(), "origin_id": i.origin_id, "origin_time": wire(i.origin_time),
                    "sender": i.sender, "sender_name": i.sender_name, "from_program": i.from_program as i64,
                    "own": i.own as i64, "thread_root": i.thread, "body": i.body, "body_hash": i.body_hash,
                    "has_link": i.has_link as i64, "has_steps": i.has_steps as i64,
                })
            })
            .collect();
        let written = sqlx::query(
            "INSERT INTO knowledge_items (item_id, source, kind, scope_id, connection, origin_id, origin_time, sender, sender_name, \
             from_program, own, thread_root, body, body_hash, language, has_link, has_steps, created_at, updated_at) \
             SELECT json_extract(value, '$.item_id'), ?1, 'chat-history', ?2, ?3, json_extract(value, '$.origin_id'), \
             json_extract(value, '$.origin_time'), json_extract(value, '$.sender'), json_extract(value, '$.sender_name'), \
             json_extract(value, '$.from_program'), json_extract(value, '$.own'), json_extract(value, '$.thread_root'), \
             json_extract(value, '$.body'), json_extract(value, '$.body_hash'), ?4, json_extract(value, '$.has_link'), \
             json_extract(value, '$.has_steps'), ?5, ?5 FROM json_each(?6) WHERE true \
             ON CONFLICT (source, scope_id, origin_id) WHERE kind = 'chat-history' DO UPDATE SET \
             origin_time = excluded.origin_time, sender = excluded.sender, sender_name = excluded.sender_name, \
             from_program = excluded.from_program, own = excluded.own, thread_root = excluded.thread_root, \
             body = excluded.body, body_hash = excluded.body_hash, language = excluded.language, \
             has_link = excluded.has_link, has_steps = excluded.has_steps, updated_at = excluded.updated_at \
             WHERE knowledge_items.body_hash <> excluded.body_hash OR knowledge_items.language <> excluded.language \
             OR knowledge_items.thread_root IS NOT excluded.thread_root OR knowledge_items.sender_name IS NOT excluded.sender_name \
             OR knowledge_items.from_program <> excluded.from_program OR knowledge_items.own <> excluded.own \
             RETURNING item_id",
        )
        .bind(source)
        .bind(scope.to_string())
        .bind(connection)
        .bind(language)
        .bind(&now)
        .bind(serde_json::Value::Array(rows).to_string())
        .fetch_all(&self.pool)
        .await?;
        Ok(written.len() as u64)
    }

    /// K2.
    async fn search(
        &self,
        source: &str,
        language: &str,
        vis: &Vis,
        query: &str,
        boost: bool,
        candidates: u32,
    ) -> Result<Vec<Ranked>, KnowledgeError> {
        let Some(q) = fts_any_word(query) else {
            return Ok(Vec::new());
        };
        let sql = if language == "english" {
            SEARCH_PORTER
        } else {
            SEARCH_PLAIN
        };
        let rows = sqlx::query(sql)
            .bind(source)
            .bind(scope_text(vis.scope))
            .bind(vis.include_own)
            .bind(hidden_json(vis))
            .bind(boost)
            .bind(q)
            .bind(i64::from(candidates))
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(Ranked {
                    item: item_from(r)?,
                    rank: r.try_get("rank")?,
                    body_hash: r.try_get("body_hash")?,
                })
            })
            .collect()
    }

    /// K3: the best-ranked reply of each matching thread.
    async fn replies(
        &self,
        source: &str,
        language: &str,
        vis: &Vis,
        query: &str,
        roots: &[String],
    ) -> Result<Vec<Ranked>, KnowledgeError> {
        let Some(q) = fts_any_word(query) else {
            return Ok(Vec::new());
        };
        let sql = if language == "english" {
            REPLIES_PORTER
        } else {
            REPLIES_PLAIN
        };
        let rows = sqlx::query(sql)
            .bind(source)
            .bind(scope_text(vis.scope))
            .bind(vis.include_own)
            .bind(hidden_json(vis))
            .bind(q)
            .bind(serde_json::to_string(roots).unwrap_or_else(|_| "[]".to_owned()))
            .fetch_all(&self.pool)
            .await?;
        let mut out: Vec<Ranked> = Vec::new();
        for r in &rows {
            let item = item_from(r)?;
            if out.iter().any(|o| o.item.thread == item.thread) {
                continue;
            }
            out.push(Ranked {
                rank: r.try_get("rank")?,
                body_hash: r.try_get("body_hash")?,
                item,
            });
        }
        Ok(out)
    }

    /// K4.
    async fn recent(
        &self,
        source: &str,
        vis: &Vis,
        limit: u32,
    ) -> Result<Vec<KnowledgeItem>, KnowledgeError> {
        let rows = sqlx::query(
            "SELECT i.* FROM knowledge_items i WHERE i.source = ?1 \
             AND (i.scope_id IS ?2 OR (i.kind = 'notes' AND i.scope_id IS NULL)) AND i.retired = 0 \
             AND (i.kind <> 'entries' OR i.verified = 1) \
             AND (i.kind <> 'chat-history' OR ((?3 OR i.own = 0) AND i.sender NOT IN (SELECT value FROM json_each(?4)))) \
             ORDER BY i.origin_time DESC, i.item_id LIMIT ?5",
        )
        .bind(source)
        .bind(scope_text(vis.scope))
        .bind(vis.include_own)
        .bind(hidden_json(vis))
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(item_from).collect()
    }

    /// K5.
    async fn open(&self, item_id: uuid::Uuid) -> Result<Option<KnowledgeItem>, KnowledgeError> {
        let row = sqlx::query("SELECT * FROM knowledge_items WHERE item_id = ?1")
            .bind(item_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(item_from).transpose()
    }

    /// K6.
    async fn senders(&self, source: &str, scope: ScopeId) -> Result<Vec<Sender>, KnowledgeError> {
        let rows = sqlx::query(
            "SELECT i.sender AS id, \
             (SELECT j.sender_name FROM knowledge_items j WHERE j.source = i.source AND j.scope_id = i.scope_id \
              AND j.kind = 'chat-history' AND j.sender = i.sender ORDER BY j.origin_time DESC LIMIT 1) AS name, \
             MAX(i.from_program) AS from_program, MAX(i.origin_time) AS last_active, COUNT(*) AS items \
             FROM knowledge_items i WHERE i.source = ?1 AND i.kind = 'chat-history' AND i.scope_id = ?2 AND i.own = 0 \
             GROUP BY i.sender ORDER BY last_active DESC, i.sender LIMIT 200",
        )
        .bind(source)
        .bind(scope.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(Sender {
                    id: r.try_get("id")?,
                    name: r.try_get("name")?,
                    from_program: r.try_get::<i64, _>("from_program")? != 0,
                    last_active: time(&r.try_get::<String, _>("last_active")?)?,
                    items: u64::try_from(r.try_get::<i64, _>("items")?).unwrap_or(0),
                })
            })
            .collect()
    }

    /// K7: the count and the insert are one statement.
    async fn note_insert(
        &self,
        item_id: uuid::Uuid,
        source: &str,
        scope: Option<ScopeId>,
        body: &str,
        language: &str,
        run_id: RunId,
        max_notes: u32,
    ) -> Result<bool, KnowledgeError> {
        let now = Timestamp::now().to_wire();
        let row = sqlx::query(
            "INSERT INTO knowledge_items (item_id, source, kind, scope_id, origin_time, own, body, body_hash, language, \
             has_link, has_steps, run_id, created_at, updated_at) \
             SELECT ?1, ?2, 'notes', ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9, ?10, ?4, ?4 \
             WHERE (SELECT COUNT(*) FROM knowledge_items n WHERE n.source = ?2 AND n.kind = 'notes' AND n.retired = 0 \
                    AND n.scope_id IS ?3) < ?11 RETURNING item_id",
        )
        .bind(item_id.to_string())
        .bind(source)
        .bind(scope_text(scope))
        .bind(&now)
        .bind(body)
        .bind(sha256_hex(body))
        .bind(language)
        .bind(has_link(body))
        .bind(has_steps(body))
        .bind(run_id.to_string())
        .bind(i64::from(max_notes))
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// K8.
    async fn note_update(
        &self,
        note_id: uuid::Uuid,
        scope: Option<ScopeId>,
        body: &str,
        run_id: RunId,
    ) -> Result<u64, KnowledgeError> {
        let now = Timestamp::now().to_wire();
        let done = sqlx::query(
            "UPDATE knowledge_items SET body = ?3, body_hash = ?4, has_link = ?5, has_steps = ?6, run_id = ?7, \
             origin_time = ?8, updated_at = ?8 WHERE item_id = ?1 AND kind = 'notes' AND retired = 0 AND scope_id IS ?2",
        )
        .bind(note_id.to_string())
        .bind(scope_text(scope))
        .bind(body)
        .bind(sha256_hex(body))
        .bind(has_link(body))
        .bind(has_steps(body))
        .bind(run_id.to_string())
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// K9.
    async fn note_retire(
        &self,
        note_id: uuid::Uuid,
        scope: Option<ScopeId>,
    ) -> Result<u64, KnowledgeError> {
        let done = sqlx::query(
            "UPDATE knowledge_items SET retired = 1, updated_at = ?3 WHERE item_id = ?1 AND kind = 'notes' AND scope_id IS ?2",
        )
        .bind(note_id.to_string())
        .bind(scope_text(scope))
        .bind(Timestamp::now().to_wire())
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// K10 / K14 (6a): `(created_at DESC, item_id DESC)`.
    async fn page(
        &self,
        kind: ItemKind,
        scope: Option<ScopeId>,
        after: Option<(String, String)>,
        limit: usize,
    ) -> Result<Vec<(KnowledgeItem, String)>, KnowledgeError> {
        let (after_at, after_id) = after.unzip();
        let rows = sqlx::query(
            "SELECT * FROM knowledge_items WHERE kind = ?1 AND scope_id IS ?2 \
             AND (?3 IS NULL OR (created_at, item_id) < (?3, ?4)) ORDER BY created_at DESC, item_id DESC LIMIT ?5",
        )
        .bind(kind.text())
        .bind(scope_text(scope))
        .bind(after_at)
        .bind(after_id)
        .bind(i64::try_from(limit).unwrap_or(101))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| Ok((item_from(r)?, r.try_get::<String, _>("created_at")?)))
            .collect()
    }

    /// K11.
    async fn entry_insert(
        &self,
        item_id: uuid::Uuid,
        source: &str,
        scope: ScopeId,
        body: &str,
        language: &str,
        by: &str,
    ) -> Result<(), KnowledgeError> {
        let now = Timestamp::now().to_wire();
        sqlx::query(
            "INSERT INTO knowledge_items (item_id, source, kind, scope_id, origin_time, body, body_hash, language, has_link, \
             has_steps, verified, added_by, created_at, updated_at) VALUES (?1, ?2, 'entries', ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?4, ?4)",
        )
        .bind(item_id.to_string())
        .bind(source)
        .bind(scope.to_string())
        .bind(&now)
        .bind(body)
        .bind(sha256_hex(body))
        .bind(language)
        .bind(has_link(body))
        .bind(has_steps(body))
        .bind(by)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// K12.
    async fn entry_verify(&self, entry_id: uuid::Uuid, by: &str) -> Result<u64, KnowledgeError> {
        let done = sqlx::query(
            "UPDATE knowledge_items SET verified = 1, verified_by = ?2, updated_at = ?3 WHERE item_id = ?1 AND kind = 'entries'",
        )
        .bind(entry_id.to_string())
        .bind(by)
        .bind(Timestamp::now().to_wire())
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// K13.
    async fn entry_remove(&self, entry_id: uuid::Uuid) -> Result<u64, KnowledgeError> {
        let done =
            sqlx::query("DELETE FROM knowledge_items WHERE item_id = ?1 AND kind = 'entries'")
                .bind(entry_id.to_string())
                .execute(&self.pool)
                .await?;
        Ok(done.rows_affected())
    }

    /// K15.
    async fn delete_older_than(
        &self,
        kind: ItemKind,
        cutoff: chrono::DateTime<chrono::Utc>,
        limit: u32,
    ) -> Result<u64, KnowledgeError> {
        let done = sqlx::query(
            "DELETE FROM knowledge_items WHERE item_id IN (SELECT item_id FROM knowledge_items \
             WHERE kind = ?1 AND origin_time < ?2 ORDER BY origin_time LIMIT ?3)",
        )
        .bind(kind.text())
        .bind(wire(cutoff))
        .bind(i64::from(limit))
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// K16.
    async fn delete_by_sender(
        &self,
        connection: &str,
        sender: &str,
    ) -> Result<u64, KnowledgeError> {
        let done = sqlx::query("DELETE FROM knowledge_items WHERE kind = 'chat-history' AND connection = ?1 AND sender = ?2")
            .bind(connection)
            .bind(sender)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    /// K17: `None` = the notes of the agent-wide scope.
    async fn delete_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError> {
        let done = sqlx::query("DELETE FROM knowledge_items WHERE scope_id IS ?1")
            .bind(scope_text(scope))
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    /// K18.
    async fn count_by_sender(&self, connection: &str, sender: &str) -> Result<u64, KnowledgeError> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_items WHERE kind = 'chat-history' AND connection = ?1 AND sender = ?2")
            .bind(connection)
            .bind(sender)
            .fetch_one(&self.pool)
            .await?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// K19.
    async fn count_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError> {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_items WHERE scope_id IS ?1")
                .bind(scope_text(scope))
                .fetch_one(&self.pool)
                .await?;
        Ok(u64::try_from(n).unwrap_or(0))
    }
}
