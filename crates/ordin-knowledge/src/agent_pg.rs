//! K1–K19 on PostgreSQL (the scaled shape): wrightrun's statements with the scope predicate on
//! `scope_id` and erasure by sender on `connection` (*Data Model*, PostgreSQL slice).

use ordin_types::{RunId, ScopeId};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::agent::{has_link, has_steps, sha256_hex, ItemStore, Prepared, Ranked, Vis};
use crate::error::KnowledgeError;
use crate::model::{ItemKind, KnowledgeItem, Sender};

pub(crate) struct PgItems {
    pub pool: PgPool,
}

/// `undefined_object` on the language (SQLSTATE 42704) is `KNOW_LANGUAGE_UNKNOWN`.
fn language_error(e: sqlx::Error, language: &str) -> KnowledgeError {
    if let sqlx::Error::Database(db) = &e {
        if db.code().as_deref() == Some("42704") {
            return KnowledgeError::LanguageUnknown {
                language: language.to_owned(),
            };
        }
    }
    KnowledgeError::from(e)
}

fn item_from(r: &PgRow) -> Result<KnowledgeItem, KnowledgeError> {
    let kind: String = r.try_get("kind")?;
    let scope: Option<uuid::Uuid> = r.try_get("scope_id")?;
    let run: Option<uuid::Uuid> = r.try_get("run_id")?;
    Ok(KnowledgeItem {
        item_id: r.try_get("item_id")?,
        source: r.try_get("source")?,
        kind: ItemKind::from_text(&kind).ok_or_else(crate::store::corrupt)?,
        scope_id: scope.map(ScopeId),
        origin_id: r.try_get("origin_id")?,
        origin_time: r.try_get("origin_time")?,
        sender: r.try_get("sender")?,
        sender_name: r.try_get("sender_name")?,
        from_program: r.try_get("from_program")?,
        own: r.try_get("own")?,
        text: r.try_get("body")?,
        thread: r.try_get("thread_root")?,
        verified: r.try_get("verified")?,
        run_id: run.map(RunId),
        retired: r.try_get("retired")?,
    })
}

fn scope_uuid(s: Option<ScopeId>) -> Option<uuid::Uuid> {
    s.map(|s| s.0)
}

#[async_trait::async_trait]
impl ItemStore for PgItems {
    /// K1.
    async fn upsert(
        &self,
        source: &str,
        scope: ScopeId,
        connection: &str,
        language: &str,
        items: &[Prepared],
    ) -> Result<u64, KnowledgeError> {
        let ids: Vec<uuid::Uuid> = items.iter().map(|i| i.item_id).collect();
        let origins: Vec<String> = items.iter().map(|i| i.origin_id.clone()).collect();
        let times: Vec<chrono::DateTime<chrono::Utc>> =
            items.iter().map(|i| i.origin_time).collect();
        let senders: Vec<String> = items.iter().map(|i| i.sender.clone()).collect();
        let names: Vec<Option<String>> = items.iter().map(|i| i.sender_name.clone()).collect();
        let programs: Vec<bool> = items.iter().map(|i| i.from_program).collect();
        let owns: Vec<bool> = items.iter().map(|i| i.own).collect();
        let threads: Vec<Option<String>> = items.iter().map(|i| i.thread.clone()).collect();
        let bodies: Vec<String> = items.iter().map(|i| i.body.clone()).collect();
        let hashes: Vec<String> = items.iter().map(|i| i.body_hash.clone()).collect();
        let links: Vec<bool> = items.iter().map(|i| i.has_link).collect();
        let steps: Vec<bool> = items.iter().map(|i| i.has_steps).collect();
        let rows = sqlx::query(
            "INSERT INTO knowledge_items (item_id, source, kind, scope_id, connection, origin_id, origin_time, sender, sender_name, \
             from_program, own, thread_root, body, body_hash, language, tsv, has_link, has_steps) \
             SELECT n.item_id, $1, 'chat-history', $2, $3, n.origin_id, n.origin_time, n.sender, n.sender_name, \
             n.from_program, n.own, n.thread_root, n.body, n.body_hash, $4, to_tsvector($4::regconfig, n.body), n.has_link, n.has_steps \
             FROM unnest($5::uuid[], $6::text[], $7::timestamptz[], $8::text[], $9::text[], $10::boolean[], $11::boolean[], \
                         $12::text[], $13::text[], $14::text[], $15::boolean[], $16::boolean[]) \
                  AS n(item_id, origin_id, origin_time, sender, sender_name, from_program, own, thread_root, body, body_hash, has_link, has_steps) \
             ON CONFLICT (source, scope_id, origin_id) WHERE kind = 'chat-history' \
             DO UPDATE SET origin_time = EXCLUDED.origin_time, sender = EXCLUDED.sender, sender_name = EXCLUDED.sender_name, \
               from_program = EXCLUDED.from_program, own = EXCLUDED.own, thread_root = EXCLUDED.thread_root, \
               body = EXCLUDED.body, body_hash = EXCLUDED.body_hash, language = EXCLUDED.language, tsv = EXCLUDED.tsv, \
               has_link = EXCLUDED.has_link, has_steps = EXCLUDED.has_steps, updated_at = now() \
             WHERE knowledge_items.body_hash <> EXCLUDED.body_hash OR knowledge_items.language <> EXCLUDED.language \
               OR knowledge_items.thread_root IS DISTINCT FROM EXCLUDED.thread_root \
               OR knowledge_items.sender_name IS DISTINCT FROM EXCLUDED.sender_name \
               OR knowledge_items.from_program <> EXCLUDED.from_program OR knowledge_items.own <> EXCLUDED.own \
             RETURNING item_id",
        )
        .bind(source)
        .bind(scope.0)
        .bind(connection)
        .bind(language)
        .bind(ids)
        .bind(origins)
        .bind(times)
        .bind(senders)
        .bind(names)
        .bind(programs)
        .bind(owns)
        .bind(threads)
        .bind(bodies)
        .bind(hashes)
        .bind(links)
        .bind(steps)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| language_error(e, language))?;
        Ok(rows.len() as u64)
    }

    /// K2 with the any-word query Q.
    async fn search(
        &self,
        source: &str,
        language: &str,
        vis: &Vis,
        query: &str,
        boost: bool,
        candidates: u32,
    ) -> Result<Vec<Ranked>, KnowledgeError> {
        let rows = sqlx::query(
            "WITH q AS (SELECT CASE WHEN numnode(p) = 0 THEN NULL ELSE replace(p::text, ' & ', ' | ')::tsquery END AS tsq \
                        FROM (SELECT plainto_tsquery($5::regconfig, $6) AS p) AS parsed) \
             SELECT i.item_id, i.source, i.kind, i.scope_id, i.origin_id, i.origin_time, i.sender, i.sender_name, \
                    i.from_program, i.own, i.body, i.body_hash, i.thread_root, i.verified, i.run_id, i.retired, \
                    (ts_rank(i.tsv, q.tsq) * CASE WHEN $7::boolean AND (i.has_link OR i.has_steps) THEN 1.5 ELSE 1.0 END)::float8 AS rank \
             FROM knowledge_items AS i, q WHERE i.source = $1 AND i.tsv @@ q.tsq \
             AND (i.scope_id IS NOT DISTINCT FROM $2::uuid OR (i.kind = 'notes' AND i.scope_id IS NULL)) AND NOT i.retired \
             AND (i.kind <> 'entries' OR i.verified) \
             AND (i.kind <> 'chat-history' OR (($3::boolean OR NOT i.own) AND NOT (i.sender = ANY($4::text[])))) \
             ORDER BY rank DESC, i.origin_time DESC, i.item_id LIMIT $8",
        )
        .bind(source)
        .bind(scope_uuid(vis.scope))
        .bind(vis.include_own)
        .bind(&vis.hidden)
        .bind(language)
        .bind(query)
        .bind(boost)
        .bind(i64::from(candidates))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| language_error(e, language))?;
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

    /// K3.
    async fn replies(
        &self,
        source: &str,
        language: &str,
        vis: &Vis,
        query: &str,
        roots: &[String],
    ) -> Result<Vec<Ranked>, KnowledgeError> {
        let rows = sqlx::query(
            "WITH q AS (SELECT CASE WHEN numnode(p) = 0 THEN NULL ELSE replace(p::text, ' & ', ' | ')::tsquery END AS tsq \
                        FROM (SELECT plainto_tsquery($5::regconfig, $6) AS p) AS parsed) \
             SELECT DISTINCT ON (i.thread_root) i.item_id, i.source, i.kind, i.scope_id, i.origin_id, i.origin_time, i.sender, \
                    i.sender_name, i.from_program, i.own, i.body, i.body_hash, i.thread_root, i.verified, i.run_id, i.retired, \
                    ts_rank(i.tsv, q.tsq)::float8 AS rank \
             FROM knowledge_items AS i, q WHERE i.source = $1 AND i.thread_root = ANY($7::text[]) AND i.origin_id <> i.thread_root \
             AND (i.scope_id IS NOT DISTINCT FROM $2::uuid OR (i.kind = 'notes' AND i.scope_id IS NULL)) AND NOT i.retired \
             AND (i.kind <> 'entries' OR i.verified) \
             AND (i.kind <> 'chat-history' OR (($3::boolean OR NOT i.own) AND NOT (i.sender = ANY($4::text[])))) \
             ORDER BY i.thread_root, rank DESC, i.origin_time DESC, i.item_id",
        )
        .bind(source)
        .bind(scope_uuid(vis.scope))
        .bind(vis.include_own)
        .bind(&vis.hidden)
        .bind(language)
        .bind(query)
        .bind(roots)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| language_error(e, language))?;
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

    /// K4.
    async fn recent(
        &self,
        source: &str,
        vis: &Vis,
        limit: u32,
    ) -> Result<Vec<KnowledgeItem>, KnowledgeError> {
        let rows = sqlx::query(
            "SELECT i.item_id, i.source, i.kind, i.scope_id, i.origin_id, i.origin_time, i.sender, i.sender_name, \
             i.from_program, i.own, i.body, i.body_hash, i.thread_root, i.verified, i.run_id, i.retired \
             FROM knowledge_items AS i WHERE i.source = $1 \
             AND (i.scope_id IS NOT DISTINCT FROM $2::uuid OR (i.kind = 'notes' AND i.scope_id IS NULL)) AND NOT i.retired \
             AND (i.kind <> 'entries' OR i.verified) \
             AND (i.kind <> 'chat-history' OR (($3::boolean OR NOT i.own) AND NOT (i.sender = ANY($4::text[])))) \
             ORDER BY i.origin_time DESC, i.item_id LIMIT $5",
        )
        .bind(source)
        .bind(scope_uuid(vis.scope))
        .bind(vis.include_own)
        .bind(&vis.hidden)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(item_from).collect()
    }

    /// K5.
    async fn open(&self, item_id: uuid::Uuid) -> Result<Option<KnowledgeItem>, KnowledgeError> {
        let row = sqlx::query(
            "SELECT i.item_id, i.source, i.kind, i.scope_id, i.origin_id, i.origin_time, i.sender, i.sender_name, \
             i.from_program, i.own, i.body, i.body_hash, i.thread_root, i.verified, i.run_id, i.retired \
             FROM knowledge_items AS i WHERE i.item_id = $1",
        )
        .bind(item_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(item_from).transpose()
    }

    /// K6.
    async fn senders(&self, source: &str, scope: ScopeId) -> Result<Vec<Sender>, KnowledgeError> {
        let rows = sqlx::query(
            "SELECT i.sender AS id, (array_agg(i.sender_name ORDER BY i.origin_time DESC))[1] AS name, \
             bool_or(i.from_program) AS from_program, max(i.origin_time) AS last_active, count(*) AS items \
             FROM knowledge_items AS i WHERE i.source = $1 AND i.kind = 'chat-history' AND i.scope_id = $2 AND NOT i.own \
             GROUP BY i.sender ORDER BY last_active DESC, i.sender LIMIT 200",
        )
        .bind(source)
        .bind(scope.0)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(Sender {
                    id: r.try_get("id")?,
                    name: r.try_get("name")?,
                    from_program: r.try_get("from_program")?,
                    last_active: r.try_get("last_active")?,
                    items: u64::try_from(r.try_get::<i64, _>("items")?).unwrap_or(0),
                })
            })
            .collect()
    }

    /// K7.
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
        let row = sqlx::query(
            "INSERT INTO knowledge_items (item_id, source, kind, scope_id, origin_time, own, body, body_hash, language, tsv, \
             has_link, has_steps, run_id) \
             SELECT $1, $2, 'notes', $3, now(), true, $4, $5, $6, to_tsvector($6::regconfig, $4), $7, $8, $9 \
             WHERE (SELECT count(*) FROM knowledge_items AS n WHERE n.source = $2 AND n.kind = 'notes' AND NOT n.retired \
                    AND n.scope_id IS NOT DISTINCT FROM $3::uuid) < $10 RETURNING item_id",
        )
        .bind(item_id)
        .bind(source)
        .bind(scope_uuid(scope))
        .bind(body)
        .bind(sha256_hex(body))
        .bind(language)
        .bind(has_link(body))
        .bind(has_steps(body))
        .bind(run_id.0)
        .bind(i64::from(max_notes))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| language_error(e, language))?;
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
        let done = sqlx::query(
            "UPDATE knowledge_items SET body = $3, body_hash = $4, tsv = to_tsvector(language::regconfig, $3), has_link = $5, \
             has_steps = $6, run_id = $7, origin_time = now(), updated_at = now() \
             WHERE item_id = $1 AND kind = 'notes' AND NOT retired AND scope_id IS NOT DISTINCT FROM $2::uuid",
        )
        .bind(note_id)
        .bind(scope_uuid(scope))
        .bind(body)
        .bind(sha256_hex(body))
        .bind(has_link(body))
        .bind(has_steps(body))
        .bind(run_id.0)
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
            "UPDATE knowledge_items SET retired = true, updated_at = now() WHERE item_id = $1 AND kind = 'notes' \
             AND scope_id IS NOT DISTINCT FROM $2::uuid",
        )
        .bind(note_id)
        .bind(scope_uuid(scope))
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// K10 / K14 (6a).
    async fn page(
        &self,
        kind: ItemKind,
        scope: Option<ScopeId>,
        after: Option<(String, String)>,
        limit: usize,
    ) -> Result<Vec<(KnowledgeItem, String)>, KnowledgeError> {
        let (after_at, after_id) = match after {
            Some((at, id)) => {
                let at = chrono::DateTime::parse_from_rfc3339(&at)
                    .map(|t| t.with_timezone(&chrono::Utc))
                    .map_err(|_| KnowledgeError::BadCursor)?;
                let id = uuid::Uuid::parse_str(&id).map_err(|_| KnowledgeError::BadCursor)?;
                (Some(at), Some(id))
            }
            None => (None, None),
        };
        let rows = sqlx::query(
            "SELECT i.item_id, i.source, i.kind, i.scope_id, i.origin_id, i.origin_time, i.sender, i.sender_name, \
             i.from_program, i.own, i.body, i.body_hash, i.thread_root, i.verified, i.run_id, i.retired, \
             to_char(i.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created \
             FROM knowledge_items AS i WHERE i.kind = $1 AND i.scope_id IS NOT DISTINCT FROM $2::uuid \
             AND ($3::timestamptz IS NULL OR (i.created_at, i.item_id) < ($3::timestamptz, $4::uuid)) \
             ORDER BY i.created_at DESC, i.item_id DESC LIMIT $5",
        )
        .bind(kind.text())
        .bind(scope_uuid(scope))
        .bind(after_at)
        .bind(after_id)
        .bind(i64::try_from(limit).unwrap_or(101))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| Ok((item_from(r)?, r.try_get::<String, _>("created")?)))
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
        sqlx::query(
            "INSERT INTO knowledge_items (item_id, source, kind, scope_id, origin_time, body, body_hash, language, tsv, \
             has_link, has_steps, verified, added_by) \
             VALUES ($1, $2, 'entries', $3, now(), $4, $5, $6, to_tsvector($6::regconfig, $4), $7, $8, false, $9)",
        )
        .bind(item_id)
        .bind(source)
        .bind(scope.0)
        .bind(body)
        .bind(sha256_hex(body))
        .bind(language)
        .bind(has_link(body))
        .bind(has_steps(body))
        .bind(by)
        .execute(&self.pool)
        .await
        .map_err(|e| language_error(e, language))?;
        Ok(())
    }

    /// K12.
    async fn entry_verify(&self, entry_id: uuid::Uuid, by: &str) -> Result<u64, KnowledgeError> {
        let done = sqlx::query(
            "UPDATE knowledge_items SET verified = true, verified_by = $2, updated_at = now() WHERE item_id = $1 AND kind = 'entries'",
        )
        .bind(entry_id)
        .bind(by)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// K13.
    async fn entry_remove(&self, entry_id: uuid::Uuid) -> Result<u64, KnowledgeError> {
        let done =
            sqlx::query("DELETE FROM knowledge_items WHERE item_id = $1 AND kind = 'entries'")
                .bind(entry_id)
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
             WHERE kind = $1 AND origin_time < $2 ORDER BY origin_time LIMIT $3 FOR UPDATE SKIP LOCKED)",
        )
        .bind(kind.text())
        .bind(cutoff)
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
        let done = sqlx::query("DELETE FROM knowledge_items WHERE kind = 'chat-history' AND connection = $1 AND sender = $2")
            .bind(connection)
            .bind(sender)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    /// K17.
    async fn delete_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError> {
        let done =
            sqlx::query("DELETE FROM knowledge_items WHERE scope_id IS NOT DISTINCT FROM $1::uuid")
                .bind(scope_uuid(scope))
                .execute(&self.pool)
                .await?;
        Ok(done.rows_affected())
    }

    /// K18.
    async fn count_by_sender(&self, connection: &str, sender: &str) -> Result<u64, KnowledgeError> {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM knowledge_items WHERE kind = 'chat-history' AND connection = $1 AND sender = $2")
            .bind(connection)
            .bind(sender)
            .fetch_one(&self.pool)
            .await?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// K19.
    async fn count_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM knowledge_items WHERE scope_id IS NOT DISTINCT FROM $1::uuid",
        )
        .bind(scope_uuid(scope))
        .fetch_one(&self.pool)
        .await?;
        Ok(u64::try_from(n).unwrap_or(0))
    }
}
