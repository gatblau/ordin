//! `GraphStore` over SQLite and PostgreSQL (ordin change M-9, memory-store amendment January 2026,
//! ADR-021): the SurrealDB store's graph behaviour on `mem_relationship_fact` and `mem_entity_name`
//! (SA-COLL-05 applies unchanged), plus the graph steps the storage's existing operations gain.
//!
//! Every caller value is bound (`exec::Stmt`); the statement text is built from constants only.

use std::collections::{BTreeMap, HashMap, HashSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value as Json;

use super::exec::{json_col, map_sql_err, SqlRow, Tx};
use super::{
    entity_cols, entity_for, entity_read_filter, fact_cols, fact_read_filter, id_collation,
    novis_filter, rel_read_filter, row_to_entity, row_to_fact, row_to_relationship,
    validity_clause, SqlDialect, SqlStorage, RELATIONSHIP_COLS,
};
use crate::store::graph::{
    check_limit, digest_ids, normalised_search, pick_hop, EntityCursor, HopEdge, ENTITY_DETAIL_MAX,
    ENTITY_PAGE_MAX, FACT_VERSIONS_MAX,
};
use crate::store::{owner_gate, owner_view};
use crate::types::api::DeletionProof;
use crate::types::domain::{Entity, Fact, Relationship};
use crate::types::ports::{
    EntityCount, EntityDetail, EntityErasureProof, EntityPage, EntitySummary, GraphStore, LinkRole,
    MemoryStore, Neighbour, StageOneQuery, StoreError,
};
use crate::types::scope::ScopeContext;

/// The `mem_entity_name` rows of an entity: its distinct normalised names (step 7′).
// `mem_entity_name.name_norm` is `CHECK (length >= 1)` and `backfill_entity_names`
// selects entities with no name row, so an entity every one of whose names normalises to "" (all
// punctuation or whitespace) would be selected forever. Such an entity gets one row holding its
// canonical name verbatim — which no normalised query phrase can equal — so the backfill terminates.
pub(super) fn entity_name_rows(canonical_name: &str, aliases: &[String]) -> Vec<String> {
    let mut names = crate::text::names_norm(canonical_name, aliases);
    if names.is_empty() {
        names.push(canonical_name.chars().take(512).collect());
    }
    names
}

/// Edges a hop reads first, per neighbour it may follow; several links often join the same two entities.
const HOP_EDGES_PER_FANOUT: usize = 8;
/// How much a hop widens that read when it did not fill the fanout.
const HOP_WIDEN: usize = 4;
/// Frontier entities one statement reads: two arms each, inside SQLite's limit of 500 per compound
/// statement.
const HOP_FRONTIER_CHUNK: usize = 100;

pub(super) fn texts(rows: &[SqlRow], col: &str) -> Result<Vec<String>, StoreError> {
    rows.iter().map(|r| r.text(col)).collect()
}

impl SqlStorage {
    /// Step 7′: replace `entity`'s `mem_entity_name` rows, inside `tx`.
    pub(super) async fn write_entity_names(
        &self,
        tx: &mut Tx,
        e: &Entity,
    ) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let p = st.text(&e.id);
        st.sql(&format!(
            "DELETE FROM mem_entity_name WHERE entity_id = {p}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        let public = entity_name_rows(&e.canonical_name, &e.public_aliases());
        let private: Vec<String> = crate::text::names_norm(&e.canonical_name, &e.private_aliases)
            .into_iter()
            .filter(|n| !public.contains(n))
            .collect();
        let rows = public
            .into_iter()
            .map(|n| (n, false))
            .chain(private.into_iter().map(|n| (n, true)));
        for (n, owner_only) in rows {
            let mut st = self.stmt();
            let vals = [
                st.text(&e.id),
                st.text(&e.owner.tenant),
                st.opt_text(e.owner.team.clone()),
                st.text(&e.collection),
                st.text(n),
                st.boolean(owner_only),
            ]
            .join(", ");
            st.sql(&format!(
                "INSERT INTO mem_entity_name (entity_id, tenant, owner_team, collection, name_norm, \
                 owner_only) VALUES ({vals}) ON CONFLICT DO NOTHING"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        Ok(())
    }

    /// Store `e`'s aliases and the folders behind them, and its name rows, inside `tx`.
    pub(super) async fn write_aliases(&self, tx: &mut Tx, e: &Entity) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let (p_aliases, p_private, p_folders, p_id, p_t) = (
            st.json(&Json::from(e.public_aliases())),
            st.json(&Json::from(e.private_aliases.clone())),
            st.json(&super::folders_json(e)?),
            st.text(&e.id),
            st.text(&e.owner.tenant),
        );
        st.sql(&format!(
            "UPDATE mem_entity SET aliases = {p_aliases}, private_aliases = {p_private}, \
             alias_folders = {p_folders} WHERE id = {p_id} AND tenant = {p_t}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        self.write_entity_names(tx, e).await
    }

    /// The names only `team`'s erased folder stated, or any of its folders when `collection` is `None`,
    /// leave the tenant's entities that outlive the erasure, inside `tx`.
    pub(super) async fn drop_folder_aliases(
        &self,
        tx: &mut Tx,
        tenant: &str,
        team: &str,
        collection: Option<&str>,
    ) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let cols = entity_cols(self.dialect);
        let (p_t, p_none) = (st.text(tenant), st.json(&Json::Array(Vec::new())));
        st.sql(&format!(
            "SELECT {cols} FROM mem_entity e WHERE e.tenant = {p_t} AND e.alias_folders <> {p_none}"
        ));
        for row in tx.fetch(st).await.map_err(map_sql_err)? {
            let mut entity = super::row_to_entity(&row)?;
            if crate::store::graph::drop_alias_folder(&mut entity, team, collection) {
                self.write_aliases(tx, &entity).await?;
            }
        }
        Ok(())
    }

    /// Remove the jobs that wrote `facts`, their parked copies and their links, inside `tx`. A worker
    /// that still holds such a job finds it gone and takes its write back.
    pub(super) async fn drop_jobs_of(
        &self,
        tx: &mut Tx,
        tenant: &str,
        facts: &[String],
    ) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let p_t = st.text(tenant);
        let named = st.in_list("fact_id", facts);
        st.sql(&format!(
            "SELECT job_id FROM mem_job_fact WHERE tenant = {p_t} AND {named}"
        ));
        let jobs = texts(&tx.fetch(st).await.map_err(map_sql_err)?, "job_id")?;
        if jobs.is_empty() {
            return Ok(());
        }
        for (table, col) in [
            ("mem_work_job", "id"),
            ("mem_dead_letter", "original_job_id"),
            ("mem_job_fact", "job_id"),
        ] {
            let mut st = self.stmt();
            let p_t = st.text(tenant);
            let named = st.in_list(col, &jobs);
            st.sql(&format!(
                "DELETE FROM {table} WHERE tenant = {p_t} AND {named}"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        Ok(())
    }

    /// Steps 11′ / 12′ inside `tx`: close every live relationship `fact_id` asserted, whose owner is
    /// the fact's owner, that no other currently valid fact of that owner asserts. A teammate's fact
    /// neither ends a link nor keeps one live. Returns how many closed.
    // spec: components/memory-store#SA-GRAPH-04
    // Only a fact of the link's owner ends it, under every scope, maintenance's included: the rule
    // follows the fact, not the caller.
    pub(super) async fn close_links(
        &self,
        tx: &mut Tx,
        tenant: &str,
        fact_id: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        let mut st = self.stmt();
        let (p_at, p_t, p_f) = (st.ts(at), st.text(tenant), st.text(fact_id));
        st.sql(&format!(
            "UPDATE mem_relationship SET valid_to = {p_at} \
             WHERE tenant = {p_t} AND valid_to IS NULL AND id IN \
             (SELECT rf.relationship_id FROM mem_relationship_fact rf \
              WHERE rf.fact_id = {p_f} AND rf.role = 'asserted') \
             AND owner_user = (SELECT f.owner_user FROM mem_fact f WHERE f.id = {p_f}) \
             AND NOT EXISTS (SELECT 1 FROM mem_relationship_fact o JOIN mem_fact f ON f.id = o.fact_id \
              WHERE o.relationship_id = mem_relationship.id AND o.role = 'asserted' \
              AND o.fact_id <> {p_f} AND f.valid_to IS NULL \
              AND f.owner_user = mem_relationship.owner_user)"
        ));
        let n = tx.execute(st).await.map_err(map_sql_err)?;
        if n > 0 {
            tracing::debug!(target: "memory", count = n, "store.links_closed");
        }
        Ok(n)
    }

    /// Step 13′ inside `tx`: of the relationships `removed` facts of their own owner have rows for
    /// (`also` excluded), those left with no asserting row from another fact of that owner and no
    /// `replaces`.
    // As `close_links`. Runs while the removed facts still exist.
    pub(super) async fn orphaned_by(
        &self,
        tx: &mut Tx,
        tenant: &str,
        removed: &[String],
        also: &HashSet<String>,
    ) -> Result<Vec<String>, StoreError> {
        if removed.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = self.stmt();
        let in_f = st.in_list("rf.fact_id", removed);
        let in_f2 = st.in_list("o.fact_id", removed);
        let p_t = st.text(tenant);
        st.sql(&format!(
            "SELECT DISTINCT r.id AS id FROM mem_relationship r \
             WHERE r.tenant = {p_t} AND r.replaces IS NULL AND r.id IN \
             (SELECT rf.relationship_id FROM mem_relationship_fact rf \
              JOIN mem_fact f ON f.id = rf.fact_id WHERE {in_f} AND f.owner_user = r.owner_user) \
             AND NOT EXISTS (SELECT 1 FROM mem_relationship_fact o JOIN mem_fact f ON f.id = o.fact_id \
              WHERE o.relationship_id = r.id AND o.role = 'asserted' AND NOT ({in_f2}) \
              AND f.owner_user = r.owner_user) ORDER BY r.id"
        ));
        let rows = tx.fetch(st).await.map_err(map_sql_err)?;
        Ok(texts(&rows, "id")?
            .into_iter()
            .filter(|r| !also.contains(r))
            .collect())
    }

    /// Step 13‴ inside `tx`: the entities and sources `facts` name, owned by those facts' owners, that
    /// nothing outside `removed` and `rels` uses, and no held record names.
    // spec: phase-1-analysis#SA-DELETE-02
    // No creator is recorded, so "the fact created it" is read as "nothing else uses it"
    // and an unused entity of the owner goes whichever write made it. A concurrent write resolving to
    // it stores a fact naming a gone id; nothing checks that here, as nothing did for `erase_entity`.
    // A source's trust goes with it. Citing the origin again starts at
    // `MEMORY_SOURCE_TRUST_DEFAULT`, so a note replaced by delete-then-write may be held, not admitted.
    pub(super) async fn unused_after(
        &self,
        tx: &mut Tx,
        tenant: &str,
        facts: &[Fact],
        removed: &[String],
        rels: &[String],
    ) -> Result<(Vec<String>, Vec<String>), StoreError> {
        let (owners, entities, sources) = crate::store::graph::delete_candidates(facts);
        let held_entity = match self.dialect {
            SqlDialect::Sqlite => {
                "EXISTS (SELECT 1 FROM json_each(q.entities) j WHERE j.value = e.id)"
            }
            SqlDialect::Postgres => "q.entities @> jsonb_build_array(e.id::text)",
        };
        let mut st = self.stmt();
        let p_t = st.text(tenant);
        let (in_e, in_o) = (
            st.in_list("e.id", &entities),
            st.in_list("e.owner_user", &owners),
        );
        let (in_f, in_r) = (st.in_list("fe.fact_id", removed), st.in_list("r.id", rels));
        st.sql(&format!(
            "SELECT e.id AS id FROM mem_entity e WHERE e.tenant = {p_t} AND {in_e} AND {in_o} \
             AND NOT EXISTS (SELECT 1 FROM mem_fact_entity fe \
              WHERE fe.entity_id = e.id AND NOT ({in_f})) \
             AND NOT EXISTS (SELECT 1 FROM mem_relationship r WHERE r.tenant = {p_t} \
              AND (r.from_ent = e.id OR r.to_ent = e.id) AND NOT ({in_r})) \
             AND NOT EXISTS (SELECT 1 FROM mem_quarantine q \
              WHERE q.tenant = {p_t} AND {held_entity}) ORDER BY e.id"
        ));
        let unused_entities = texts(&tx.fetch(st).await.map_err(map_sql_err)?, "id")?;
        let mut st = self.stmt();
        let p_t = st.text(tenant);
        let (in_s, in_o) = (
            st.in_list("s.id", &sources),
            st.in_list("s.owner_user", &owners),
        );
        let (in_f, in_r) = (st.in_list("f.id", removed), st.in_list("r.id", rels));
        st.sql(&format!(
            "SELECT s.id AS id FROM mem_source s WHERE s.tenant = {p_t} AND {in_s} AND {in_o} \
             AND NOT EXISTS (SELECT 1 FROM mem_fact f \
              WHERE f.tenant = {p_t} AND f.source_id = s.id AND NOT ({in_f})) \
             AND NOT EXISTS (SELECT 1 FROM mem_relationship r \
              WHERE r.tenant = {p_t} AND r.source_id = s.id AND NOT ({in_r})) \
             AND NOT EXISTS (SELECT 1 FROM mem_quarantine q \
              WHERE q.tenant = {p_t} AND q.source_id = s.id) ORDER BY s.id"
        ));
        let unused_sources = texts(&tx.fetch(st).await.map_err(map_sql_err)?, "id")?;
        Ok((unused_entities, unused_sources))
    }

    /// Delete `entities` with their name rows and `sources`, inside `tx`.
    pub(super) async fn delete_unused(
        &self,
        tx: &mut Tx,
        tenant: &str,
        entities: &[String],
        sources: &[String],
    ) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let in_e = st.in_list("entity_id", entities);
        st.sql(&format!("DELETE FROM mem_entity_name WHERE {in_e}"));
        tx.execute(st).await.map_err(map_sql_err)?;
        for (table, ids) in [("mem_entity", entities), ("mem_source", sources)] {
            let mut st = self.stmt();
            let (in_ids, p_t) = (st.in_list("id", ids), st.text(tenant));
            st.sql(&format!(
                "DELETE FROM {table} WHERE {in_ids} AND tenant = {p_t}"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        Ok(())
    }

    /// Delete `rels` and every fact row naming them or `facts`, inside `tx`.
    pub(super) async fn delete_links(
        &self,
        tx: &mut Tx,
        tenant: &str,
        facts: &[String],
        rels: &[String],
    ) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let in_f = st.in_list("fact_id", facts);
        let in_r = st.in_list("relationship_id", rels);
        st.sql(&format!(
            "DELETE FROM mem_relationship_fact WHERE {in_f} OR {in_r}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        let mut st = self.stmt();
        let in_r = st.in_list("id", rels);
        let p_t = st.text(tenant);
        st.sql(&format!(
            "DELETE FROM mem_relationship WHERE {in_r} AND tenant = {p_t}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        Ok(())
    }

    /// Whether `tenant`'s names are backfilled (SA-GRAPH-12); cached once set.
    pub(super) async fn names_backfilled(&self, tenant: &str) -> Result<bool, StoreError> {
        if self
            .backfilled
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .contains(tenant)
        {
            return Ok(true);
        }
        let mut st = self.stmt();
        let p = st.text(tenant);
        st.sql(&format!(
            "SELECT names_backfilled_at FROM mem_maintenance_state WHERE tenant = {p}"
        ));
        let rows = self.fetch(st).await?;
        let set = match rows.first() {
            Some(r) => r.opt_ts("names_backfilled_at")?.is_some(),
            None => false,
        };
        if set {
            self.backfilled
                .write()
                .unwrap_or_else(|p| p.into_inner())
                .insert(tenant.to_string());
        }
        Ok(set)
    }

    /// Steps 9″.1, 9″.2 and the neighbour walk of 9″.4 (as the SurrealDB store's `graph_reach`).
    pub(super) async fn graph_reach(
        &self,
        ctx: &ScopeContext,
        q: &StageOneQuery,
    ) -> Result<Option<crate::store::graph::Reach>, StoreError> {
        let seeds: Vec<String> = match &q.filters.entity {
            Some(e) => vec![e.clone()],
            None => {
                if !self.names_backfilled(&ctx.tenant).await? {
                    if !self
                        .graph_unavailable_logged
                        .swap(true, std::sync::atomic::Ordering::Relaxed)
                    {
                        tracing::info!(target: "memory", "store.recall_graph_unavailable");
                    }
                    return Ok(None);
                }
                let phrases = crate::text::query_phrases(&q.phrase_texts);
                if phrases.is_empty() {
                    return Ok(None);
                }
                self.match_entities(
                    ctx,
                    &phrases,
                    q.filters.collections.as_deref(),
                    crate::store::graph::MAX_SEEDS,
                )
                .await?
                .into_iter()
                .map(|e| e.id)
                .collect()
            }
        };
        if seeds.is_empty() {
            return Ok(None);
        }
        let neighbours = if q.graph_hops >= 1 {
            self.neighbours(
                ctx,
                &seeds,
                q.graph_hops.min(2),
                q.graph_fanout.max(1),
                q.filters.valid_at,
            )
            .await?
        } else {
            Vec::new()
        };
        Ok(Some(crate::store::graph::Reach::new(seeds, neighbours)))
    }

    /// The edges one hop considers, best first: the readable, valid relationships touching
    /// `frontier`, in `pick_hop`'s order, at most `limit`, and whether any were left out.
    // spec: components/memory-store#SA-GRAPH-05
    // A hub entity touches thousands of relationships, and only the first `fanout` new endpoints in
    // confidence-then-id order count. `pick_hop` takes those greedily, so the head of that order decides it: each end of each frontier
    // entity is read through its ranked index up to `limit`, and any row of the first `limit` overall
    // is among them. The caller widens `limit` until `pick_hop` fills the fanout or nothing was left
    // out, so the result is that of considering every edge.
    async fn hop_edges(
        &self,
        ctx: &ScopeContext,
        frontier: &[String],
        valid_at: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<(Vec<HopEdge>, bool), StoreError> {
        const COLS: &str = "id, from_ent, to_ent, confidence, replaces";
        let collate = id_collation(self.dialect);
        let mut edges: Vec<HopEdge> = Vec::new();
        let mut truncated = false;
        for chunk in frontier.chunks(HOP_FRONTIER_CHUNK) {
            let mut st = self.stmt();
            // SA-GRAPH-05 reads the owner here; a correction held to its owner stays with it.
            let p_u = st.text(&ctx.user);
            let read = format!(
                "{} AND (NOT r.owner_only OR r.owner_user = {p_u})",
                novis_filter(&mut st, ctx, "r")
            );
            let validity = validity_clause(&mut st, valid_at).replace("f.", "r.");
            let p_l = st.int(i64::try_from(limit).unwrap_or(i64::MAX));
            let mut arms: Vec<String> = Vec::with_capacity(chunk.len() * 2);
            for entity in chunk {
                let p_e = st.text(entity);
                for col in ["from_ent", "to_ent"] {
                    let n = arms.len();
                    arms.push(format!(
                        "SELECT {COLS} FROM (SELECT r.id AS id, r.from_ent AS from_ent, \
                         r.to_ent AS to_ent, r.confidence AS confidence, r.replaces AS replaces \
                         FROM mem_relationship r WHERE r.{col} = {p_e} AND ({read}) AND {validity} \
                         ORDER BY r.confidence DESC, r.id{collate} LIMIT {p_l}) AS arm{n}"
                    ));
                }
            }
            let arms = arms.join(" UNION ALL ");
            st.sql(&format!(
                "SELECT {COLS} FROM ({arms}) AS edge \
                 ORDER BY confidence DESC, id{collate} LIMIT {p_l}"
            ));
            let rows = self.fetch(st).await?;
            truncated |= rows.len() >= limit;
            for r in rows {
                edges.push(HopEdge {
                    id: r.text("id")?,
                    from: r.text("from_ent")?,
                    to: r.text("to_ent")?,
                    confidence: r.float("confidence")?,
                    replaces: r.opt_text("replaces")?,
                });
            }
        }
        // A link between two frontier entities is read from both of its ends.
        edges.sort_by(HopEdge::rank);
        edges.dedup_by(|a, b| a.id == b.id);
        edges.truncate(limit);
        Ok((edges, truncated))
    }

    /// Step 9″.7: each cited source's trust, read-filtered.
    pub(super) async fn source_trusts(
        &self,
        ctx: &ScopeContext,
        ids: &[String],
    ) -> Result<HashMap<String, f64>, StoreError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        // Every id comes from a fact the read filter admitted, and its source belongs to that fact's
        // owner: a reader outside the owner's team must see the same trust as the owner, not the 1.0
        // an unreadable source would read as.
        let mut st = self.stmt();
        let in_ids = st.in_list("s.id", ids);
        let p_t = st.text(&ctx.tenant);
        st.sql(&format!(
            "SELECT s.id AS id, s.trust_signal AS trust_signal FROM mem_source s \
             WHERE {in_ids} AND s.tenant = {p_t}"
        ));
        self.fetch(st)
            .await?
            .iter()
            .map(|r| Ok((r.text("id")?, r.float("trust_signal")?)))
            .collect()
    }

    /// Readable relationships touching any of `entities`, of any validity unless `live`.
    async fn relationships_touching(
        &self,
        ctx: &ScopeContext,
        entities: &[String],
        live: bool,
    ) -> Result<Vec<Relationship>, StoreError> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = self.stmt();
        let from = st.in_list("r.from_ent", entities);
        let to = st.in_list("r.to_ent", entities);
        let read = rel_read_filter(&mut st, ctx, "r");
        let validity = if live { " AND r.valid_to IS NULL" } else { "" };
        st.sql(&format!(
            "SELECT {RELATIONSHIP_COLS} FROM mem_relationship r \
             WHERE ({from} OR {to}) AND ({read}){validity}"
        ));
        self.fetch(st)
            .await?
            .iter()
            .map(row_to_relationship)
            .collect()
    }

    /// The caller's writes still on their way that name `entity`: its queued and failed jobs, and its
    /// held records, read in Rust because a name lives inside each payload.
    // Only the caller's. Another user's queued write naming the entity writes that user's
    // own entity, and is not the caller's to remove.
    async fn writes_naming(
        &self,
        ctx: &ScopeContext,
        entity: &Entity,
    ) -> Result<crate::store::graph::PendingWrites, StoreError> {
        let mut names: HashSet<String> =
            crate::text::names_norm(&entity.canonical_name, &entity.aliases)
                .into_iter()
                .collect();
        names.remove("");
        let (user_key, payload) = (
            match self.dialect {
                SqlDialect::Sqlite => "json_extract(scope, '$.user')",
                SqlDialect::Postgres => "scope->>'user'",
            },
            json_col(self.dialect, "payload", "payload"),
        );
        let mut out = crate::store::graph::PendingWrites::default();
        for (table, who) in [
            ("mem_work_job", "scope_user"),
            ("mem_dead_letter", user_key),
        ] {
            let mut st = self.stmt();
            let (p_t, p_u) = (st.text(&ctx.tenant), st.text(&ctx.user));
            st.sql(&format!(
                "SELECT id, {payload} FROM {table} WHERE tenant = {p_t} AND {who} = {p_u}"
            ));
            for r in self.fetch(st).await? {
                if crate::store::payload_names(&r.json("payload")?, &names) {
                    let slot = if table == "mem_work_job" {
                        &mut out.jobs
                    } else {
                        &mut out.dead
                    };
                    slot.push(r.text("id")?);
                }
            }
        }
        let mut st = self.stmt();
        let (p_t, p_u) = (st.text(&ctx.tenant), st.text(&ctx.user));
        let cols = ["content", "entities", "entities_asserted", "links_asserted"]
            .map(|c| json_col(self.dialect, c, c))
            .join(", ");
        st.sql(&format!(
            "SELECT id, {cols} FROM mem_quarantine WHERE tenant = {p_t} AND owner_user = {p_u}"
        ));
        for r in self.fetch(st).await? {
            let resolved = r.json("entities")?;
            let names_it = resolved
                .as_array()
                .is_some_and(|a| a.iter().any(|e| e.as_str() == Some(&entity.id)));
            let held = crate::store::names_in_write(
                &r.json("content")?,
                &r.json("entities_asserted")?,
                &r.json("links_asserted")?,
            );
            if names_it || !held.is_disjoint(&names) {
                out.held.push(r.text("id")?);
            }
        }
        Ok(out)
    }

    /// Readable facts naming `entity`, newest first, any validity unless `live`.
    async fn facts_naming(
        &self,
        ctx: &ScopeContext,
        entity: &str,
        live: bool,
        limit: Option<u32>,
    ) -> Result<Vec<Fact>, StoreError> {
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        let p_e = st.text(entity);
        let read = fact_read_filter(&mut st, ctx);
        let validity = if live { " AND f.valid_to IS NULL" } else { "" };
        let lim = match limit {
            Some(l) => format!(" LIMIT {}", st.int(i64::from(l))),
            None => String::new(),
        };
        let collate = id_collation(self.dialect);
        st.sql(&format!(
            "SELECT {cols} FROM mem_fact f WHERE f.id IN \
             (SELECT fe.fact_id FROM mem_fact_entity fe WHERE fe.entity_id = {p_e}) \
             AND ({read}){validity} ORDER BY f.ingested_at DESC, f.id{collate} ASC{lim}"
        ));
        self.fetch(st).await?.iter().map(row_to_fact).collect()
    }
}

#[async_trait]
impl GraphStore for SqlStorage {
    async fn find_live_relationship(
        &self,
        ctx: &ScopeContext,
        from: &str,
        kind: &str,
        to: &str,
    ) -> Result<Option<Relationship>, StoreError> {
        let mut st = self.stmt();
        let team = match ctx.teams.first() {
            Some(t) => {
                let p = st.text(t);
                format!("r.owner_team = {p}")
            }
            None => "r.owner_team IS NULL".to_string(),
        };
        let (p_t, p_f, p_k, p_to) = (
            st.text(&ctx.tenant),
            st.text(from),
            st.text(kind),
            st.text(to),
        );
        let p_u = st.text(&ctx.user);
        let read = rel_read_filter(&mut st, ctx, "r");
        let collate = id_collation(self.dialect);
        st.sql(&format!(
            "SELECT {RELATIONSHIP_COLS} FROM mem_relationship r WHERE r.tenant = {p_t} AND {team} \
             AND r.from_ent = {p_f} AND r.kind = {p_k} AND r.to_ent = {p_to} \
             AND r.valid_to IS NULL AND ({read}) \
             ORDER BY CASE WHEN r.owner_user = {p_u} THEN 0 ELSE 1 END, r.id{collate} LIMIT 1"
        ));
        self.fetch(st)
            .await?
            .first()
            .map(row_to_relationship)
            .transpose()
    }

    async fn relationship_kinds(
        &self,
        ctx: &ScopeContext,
        limit: u32,
    ) -> Result<Vec<String>, StoreError> {
        let mut st = self.stmt();
        let read = rel_read_filter(&mut st, ctx, "r");
        let p_l = st.int(i64::from(limit));
        st.sql(&format!(
            "SELECT r.kind AS kind FROM mem_relationship r WHERE ({read}) \
             GROUP BY r.kind ORDER BY MAX(r.ingested_at) DESC, r.kind ASC LIMIT {p_l}"
        ));
        texts(&self.fetch(st).await?, "kind")
    }

    async fn link_fact(
        &self,
        _ctx: &ScopeContext,
        relationship_id: &str,
        fact_id: &str,
        role: LinkRole,
    ) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let vals = [
            st.text(relationship_id),
            st.text(fact_id),
            st.text(role.as_str()),
            st.ts(Utc::now()),
        ]
        .join(", ");
        st.sql(&format!(
            "INSERT INTO mem_relationship_fact (relationship_id, fact_id, role, at) VALUES ({vals}) \
             ON CONFLICT (relationship_id, fact_id, role) DO UPDATE SET at = excluded.at"
        ));
        self.execute(st).await?;
        Ok(())
    }

    async fn link_is_shared(&self, ctx: &ScopeContext, id: &str) -> Result<bool, StoreError> {
        let mut st = self.stmt();
        let (p_id, p_t) = (st.text(id), st.text(&ctx.tenant));
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM mem_relationship r WHERE r.id = {p_id} AND r.tenant = {p_t} \
             AND ((r.replaces IS NOT NULL AND NOT r.owner_only) OR EXISTS \
              (SELECT 1 FROM mem_relationship_fact lf JOIN mem_fact f ON f.id = lf.fact_id \
               WHERE lf.relationship_id = r.id AND f.visibility <> 'user-private'))"
        ));
        Ok(super::count_of(&self.fetch(st).await?)? > 0)
    }

    async fn links_of_fact(
        &self,
        ctx: &ScopeContext,
        fact_id: &str,
    ) -> Result<Vec<(Relationship, LinkRole)>, StoreError> {
        let mut st = self.stmt();
        let p_f = st.text(fact_id);
        let read = rel_read_filter(&mut st, ctx, "r");
        let collate = id_collation(self.dialect);
        st.sql(&format!(
            "SELECT {RELATIONSHIP_COLS}, rf.role AS link_role FROM mem_relationship_fact rf \
             JOIN mem_relationship r ON r.id = rf.relationship_id \
             WHERE rf.fact_id = {p_f} AND ({read}) ORDER BY r.id{collate}, rf.role"
        ));
        self.fetch(st)
            .await?
            .iter()
            .map(|row| {
                let role = LinkRole::parse(&row.text("link_role")?)
                    .ok_or_else(|| StoreError::Internal("decode link role".into()))?;
                Ok((row_to_relationship(row)?, role))
            })
            .collect()
    }

    async fn match_entities(
        &self,
        ctx: &ScopeContext,
        phrases: &[String],
        collections: Option<&[String]>,
        limit: u32,
    ) -> Result<Vec<Entity>, StoreError> {
        if phrases.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut st = self.stmt();
        let cols = entity_cols(self.dialect);
        let p_t = st.text(&ctx.tenant);
        let in_p = st.in_list("n.name_norm", phrases);
        let p_u = st.text(&ctx.user);
        let read = entity_read_filter(&mut st, ctx, "e");
        let coll = match collections {
            Some(c) => format!(" AND {}", st.in_list("e.collection", c)),
            None => String::new(),
        };
        // As in the SurrealDB store: every match is read and the longest-phrase order
        // and the limit applied here.
        st.sql(&format!(
            "SELECT {cols}, n.name_norm AS matched FROM mem_entity_name n \
             JOIN mem_entity e ON e.id = n.entity_id \
             WHERE n.tenant = {p_t} AND {in_p} AND (NOT n.owner_only OR e.owner_user = {p_u}) \
             AND ({read}){coll}"
        ));
        let mut best: BTreeMap<String, (usize, Entity)> = BTreeMap::new();
        for row in self.fetch(st).await? {
            let len = row.text("matched")?.chars().count();
            let e = entity_for(ctx, &row)?;
            let slot = best.entry(e.id.clone()).or_insert((len, e));
            slot.0 = slot.0.max(len);
        }
        let mut scored: Vec<(usize, Entity)> = best.into_values().collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        scored.truncate(limit as usize);
        tracing::debug!(target: "memory", phrases = phrases.len(), matched = scored.len(), "store.match_entities");
        Ok(scored.into_iter().map(|(_, e)| e).collect())
    }

    async fn neighbours(
        &self,
        ctx: &ScopeContext,
        seeds: &[String],
        hops: u8,
        fanout: u32,
        valid_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<Neighbour>, StoreError> {
        if !(1..=2).contains(&hops) {
            return Err(StoreError::Validation("hops out of range".into()));
        }
        let mut seen: HashSet<String> = seeds.iter().cloned().collect();
        let mut frontier: HashSet<String> = seeds.iter().cloned().collect();
        let mut out = Vec::new();
        for hop in 1..=hops {
            if frontier.is_empty() {
                break;
            }
            let f: Vec<String> = frontier.iter().cloned().collect();
            let mut limit = (fanout as usize).saturating_mul(HOP_EDGES_PER_FANOUT);
            let picked = loop {
                let (edges, truncated) = self.hop_edges(ctx, &f, valid_at, limit).await?;
                if edges.is_empty() {
                    break Vec::new();
                }
                // SA-GRAPH-05: which have an asserting row, and which an asserting fact the caller
                // can read and that is valid at the same time.
                let ids: Vec<String> = edges.iter().map(|e| e.id.clone()).collect();
                let mut st = self.stmt();
                let in_r = st.in_list("rf.relationship_id", &ids);
                let read = fact_read_filter(&mut st, ctx);
                let validity = validity_clause(&mut st, valid_at);
                st.sql(&format!(
                    "SELECT rf.relationship_id AS id, \
                     CASE WHEN EXISTS (SELECT 1 FROM mem_fact f WHERE f.id = rf.fact_id \
                       AND ({read}) AND {validity}) THEN 1 ELSE 0 END AS ok \
                     FROM mem_relationship_fact rf WHERE {in_r} AND rf.role = 'asserted'"
                ));
                let mut has_asserted = HashSet::new();
                let mut readable = HashSet::new();
                for r in self.fetch(st).await? {
                    let id = r.text("id")?;
                    if r.int("ok")? == 1 {
                        readable.insert(id.clone());
                    }
                    has_asserted.insert(id);
                }
                let picked = pick_hop(&frontier, edges, &has_asserted, &readable, &seen, fanout);
                if picked.len() as u32 >= fanout || !truncated {
                    break picked;
                }
                limit = limit.saturating_mul(HOP_WIDEN);
            };
            if picked.is_empty() {
                break;
            }
            frontier = HashSet::new();
            for (entity, via) in picked {
                seen.insert(entity.clone());
                frontier.insert(entity.clone());
                out.push(Neighbour {
                    entity_id: entity,
                    hop,
                    via,
                });
            }
        }
        tracing::debug!(target: "memory", seeds = seeds.len(), reached = out.len(), hops, "store.neighbours");
        Ok(out)
    }

    async fn list_entities(
        &self,
        ctx: &ScopeContext,
        search: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<EntityPage, StoreError> {
        check_limit(limit, ENTITY_PAGE_MAX, "limit")?;
        let search = normalised_search(search)?;
        let after = cursor.map(EntityCursor::decode).transpose()?;
        let collate = id_collation(self.dialect);
        let mut st = self.stmt();
        let cols = entity_cols(self.dialect);
        let read = entity_read_filter(&mut st, ctx, "e");
        let keyset = match &after {
            Some(c) => {
                let (p_n, p_id) = (st.text(&c.n), st.text(&c.id));
                format!(
                    " AND (e.canonical_name{collate} > {p_n} \
                     OR (e.canonical_name = {p_n} AND e.id{collate} > {p_id}))"
                )
            }
            None => String::new(),
        };
        let filter = match &search {
            Some(s) => {
                let p = st.text(s);
                let contains = match self.dialect {
                    SqlDialect::Sqlite => format!("instr(n.name_norm, {p}) > 0"),
                    SqlDialect::Postgres => format!("strpos(n.name_norm, {p}) > 0"),
                };
                let p_u = st.text(&ctx.user);
                format!(
                    " AND EXISTS (SELECT 1 FROM mem_entity_name n WHERE n.entity_id = e.id \
                     AND {contains} AND (NOT n.owner_only OR e.owner_user = {p_u}))"
                )
            }
            None => String::new(),
        };
        let p_l = st.int(i64::from(limit) + 1);
        st.sql(&format!(
            "SELECT {cols} FROM mem_entity e WHERE ({read}){keyset}{filter} \
             ORDER BY e.canonical_name{collate} ASC, e.id{collate} ASC LIMIT {p_l}"
        ));
        let mut entities: Vec<Entity> = self
            .fetch(st)
            .await?
            .iter()
            .map(|r| entity_for(ctx, r))
            .collect::<Result<_, _>>()?;
        let next_cursor = if entities.len() > limit as usize {
            entities.truncate(limit as usize);
            entities.last().map(|e| {
                EntityCursor {
                    n: e.canonical_name.clone(),
                    id: e.id.clone(),
                }
                .encode()
            })
        } else {
            None
        };
        let ids: Vec<String> = entities.iter().map(|e| e.id.clone()).collect();
        let mut fact_count: HashMap<String, u32> = HashMap::new();
        let mut link_count: HashMap<String, u32> = HashMap::new();
        if !ids.is_empty() {
            let mut st = self.stmt();
            let in_e = st.in_list("fe.entity_id", &ids);
            let read = fact_read_filter(&mut st, ctx);
            st.sql(&format!(
                "SELECT fe.entity_id AS id, COUNT(*) AS n FROM mem_fact_entity fe \
                 JOIN mem_fact f ON f.id = fe.fact_id \
                 WHERE {in_e} AND ({read}) AND f.valid_to IS NULL GROUP BY fe.entity_id"
            ));
            for r in self.fetch(st).await? {
                fact_count.insert(r.text("id")?, u32::try_from(r.int("n")?).unwrap_or(0));
            }
            for r in self.relationships_touching(ctx, &ids, true).await? {
                *link_count.entry(r.from.clone()).or_default() += 1;
                if r.to != r.from {
                    *link_count.entry(r.to.clone()).or_default() += 1;
                }
            }
        }
        Ok(EntityPage {
            items: entities
                .into_iter()
                .map(|e| EntitySummary {
                    fact_count: fact_count.get(&e.id).copied().unwrap_or(0),
                    link_count: link_count.get(&e.id).copied().unwrap_or(0),
                    entity: e,
                })
                .collect(),
            next_cursor,
        })
    }

    async fn entity_detail(
        &self,
        ctx: &ScopeContext,
        entity_id: &str,
        limit: u32,
    ) -> Result<Option<EntityDetail>, StoreError> {
        check_limit(limit, ENTITY_DETAIL_MAX, "limit")?;
        let Some(entity) = self.get_entity(ctx, entity_id).await? else {
            return Ok(None);
        };
        let facts = self.facts_naming(ctx, entity_id, true, Some(limit)).await?;
        let mut links = self
            .relationships_touching(ctx, std::slice::from_ref(&entity.id), false)
            .await?;
        links.sort_by(|a, b| {
            b.valid_from
                .cmp(&a.valid_from)
                .then_with(|| a.id.cmp(&b.id))
        });
        links.truncate(limit as usize);
        let mut names: HashMap<String, String> = HashMap::new();
        names.insert(entity.id.clone(), entity.canonical_name.clone());
        let ends: Vec<String> = links
            .iter()
            .flat_map(|r| [r.from.clone(), r.to.clone()])
            .filter(|e| !names.contains_key(e))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if !ends.is_empty() {
            let mut st = self.stmt();
            let in_e = st.in_list("e.id", &ends);
            let read = entity_read_filter(&mut st, ctx, "e");
            st.sql(&format!(
                "SELECT e.id AS id, e.canonical_name AS canonical_name FROM mem_entity e \
                 WHERE {in_e} AND ({read})"
            ));
            for r in self.fetch(st).await? {
                names.insert(r.text("id")?, r.text("canonical_name")?);
            }
        }
        Ok(Some(EntityDetail {
            entity,
            facts,
            links,
            names,
        }))
    }

    async fn fact_versions(
        &self,
        ctx: &ScopeContext,
        fact_id: &str,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError> {
        check_limit(limit, FACT_VERSIONS_MAX, "limit")?;
        crate::store::graph::versions_through(self, ctx, fact_id, limit).await
    }

    async fn keep_fact(
        &self,
        ctx: &ScopeContext,
        fact_id: &str,
        salience_floor: f64,
    ) -> Result<(), StoreError> {
        let Some(f) = self.get_fact(ctx, fact_id).await? else {
            return Err(StoreError::NotFound);
        };
        if f.valid_to.is_some() {
            return Err(StoreError::NotFound);
        }
        owner_gate(ctx, &f.owner.user)?;
        let mut st = self.stmt();
        let (p_s, p_id, p_t) = (
            st.float(f.salience.max(salience_floor).clamp(0.0, 1.0)),
            st.text(fact_id),
            st.text(&ctx.tenant),
        );
        st.sql(&format!(
            "UPDATE mem_fact SET memory_class = 'semantic', salience = {p_s} \
             WHERE id = {p_id} AND tenant = {p_t}"
        ));
        self.execute(st).await?;
        // `memory_class` is a filter column of the search cache.
        self.embeddings.forget(&ctx.tenant, &[fact_id.to_string()]);
        tracing::debug!(target: "memory", "store.keep_fact");
        Ok(())
    }

    async fn count_entity(
        &self,
        ctx: &ScopeContext,
        entity_id: &str,
    ) -> Result<EntityCount, StoreError> {
        if self.get_entity(ctx, entity_id).await?.is_none() {
            return Err(StoreError::NotFound);
        }
        let facts = self.facts_naming(ctx, entity_id, false, None).await?.len();
        let links = self
            .relationships_touching(ctx, &[entity_id.to_string()], false)
            .await?
            .len();
        Ok(EntityCount {
            facts: u32::try_from(facts).unwrap_or(u32::MAX),
            links: u32::try_from(links).unwrap_or(u32::MAX),
        })
    }

    async fn erase_entity(
        &self,
        ctx: &ScopeContext,
        entity_id: &str,
    ) -> Result<EntityErasureProof, StoreError> {
        let Some(entity) = self.get_entity(ctx, entity_id).await? else {
            return Err(StoreError::NotFound);
        };
        owner_gate(ctx, &entity.owner.user)?;
        // The caller's own facts (SA-AUTHZ-07). One of anyone else's keeps the entity, and is reported.
        let facts: Vec<String> = self
            .facts_naming(ctx, entity_id, false, None)
            .await?
            .into_iter()
            .filter(|f| owner_view(ctx, &f.owner.user))
            .map(|f| f.id)
            .collect();
        let derived: Vec<String> = if facts.is_empty() {
            Vec::new()
        } else {
            let mut st = self.stmt();
            let in_d = st.in_list("d.derived_from", &facts);
            let read = fact_read_filter(&mut st, ctx);
            st.sql(&format!(
                "SELECT f.id AS id, f.owner_user AS owner_user FROM mem_fact f WHERE f.id IN \
                 (SELECT d.fact_id FROM mem_fact_derived d WHERE {in_d}) AND ({read})"
            ));
            let named: HashSet<&String> = facts.iter().collect();
            let mut ids = Vec::new();
            for r in self.fetch(st).await? {
                let id = r.text("id")?;
                if !named.contains(&id) && owner_view(ctx, &r.text("owner_user")?) {
                    ids.push(id);
                }
            }
            ids
        };
        let touching: Vec<String> = self
            .relationships_touching(ctx, &[entity_id.to_string()], false)
            .await?
            .into_iter()
            .filter(|r| owner_view(ctx, &r.owner.user))
            .map(|r| r.id)
            .collect();
        let writes = self.writes_naming(ctx, &entity).await?;
        let mut removed = facts.clone();
        removed.extend(derived.iter().cloned());

        let mut tx = self.begin().await?;
        // The writes on their way first, as `erase_collection`.
        for (table, ids) in [
            ("mem_work_job", &writes.jobs),
            ("mem_dead_letter", &writes.dead),
            ("mem_quarantine", &writes.held),
        ] {
            let mut st = self.stmt();
            let in_ids = st.in_list("id", ids);
            let p_t = st.text(&ctx.tenant);
            st.sql(&format!(
                "DELETE FROM {table} WHERE {in_ids} AND tenant = {p_t}"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        let orphans = self
            .orphaned_by(
                &mut tx,
                &ctx.tenant,
                &removed,
                &touching.iter().cloned().collect(),
            )
            .await?;
        let mut rels = touching.clone();
        rels.extend(orphans);
        self.delete_links(&mut tx, &ctx.tenant, &removed, &rels)
            .await?;
        for table in ["mem_fact_entity", "mem_fact_derived"] {
            let mut st = self.stmt();
            let in_f = st.in_list("fact_id", &removed);
            st.sql(&format!("DELETE FROM {table} WHERE {in_f}"));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        let mut st = self.stmt();
        let in_f = st.in_list("id", &removed);
        let p_t = st.text(&ctx.tenant);
        st.sql(&format!(
            "DELETE FROM mem_fact WHERE {in_f} AND tenant = {p_t}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        // Re-count before committing: a row of the removal set remaining is partial; what other users
        // hold is the outcome (SA-GRAPH-13), read past the read filter so a row the caller cannot see
        // still keeps the entity.
        // spec: phase-1-analysis#SA-GRAPH-13
        // The counts tell an owner that rows it cannot read name its entity. They carry
        // no id, owner or content, and the entity's name stays while they do.
        let mut st = self.stmt();
        let in_f = st.in_list("f.id", &removed);
        let p_t = st.text(&ctx.tenant);
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM mem_fact f WHERE {in_f} AND f.tenant = {p_t}"
        ));
        let survived = super::count_of(&tx.fetch(st).await.map_err(map_sql_err)?)?;
        if survived > 0 {
            let expected = u32::try_from(removed.len()).unwrap_or(u32::MAX);
            return Err(StoreError::PartialDelete {
                removed: expected.saturating_sub(u32::try_from(survived).unwrap_or(u32::MAX)),
                expected,
            });
        }
        let mut st = self.stmt();
        let (p, p_t) = (st.text(entity_id), st.text(&ctx.tenant));
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM mem_fact_entity fe JOIN mem_fact f ON f.id = fe.fact_id \
             WHERE fe.entity_id = {p} AND f.tenant = {p_t}"
        ));
        let facts_left = super::count_of(&tx.fetch(st).await.map_err(map_sql_err)?)?;
        let mut st = self.stmt();
        let (p, p_t) = (st.text(entity_id), st.text(&ctx.tenant));
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM mem_relationship r \
             WHERE r.tenant = {p_t} AND (r.from_ent = {p} OR r.to_ent = {p})"
        ));
        let links_left = super::count_of(&tx.fetch(st).await.map_err(map_sql_err)?)?;
        let entity_removed = facts_left == 0 && links_left == 0;
        if entity_removed {
            self.delete_unused(&mut tx, &ctx.tenant, &[entity_id.to_string()], &[])
                .await?;
        }
        tx.commit().await.map_err(map_sql_err)?;
        self.embeddings.forget(&ctx.tenant, &removed);
        let mut ids = removed.clone();
        ids.extend(rels.iter().cloned());
        self.sweep_summaries(ctx, &removed).await?;
        if entity_removed {
            ids.push(entity_id.to_string());
        }
        ids.extend(writes.all());
        let proof = EntityErasureProof {
            entity_removed,
            facts_remaining: u32::try_from(facts_left).unwrap_or(u32::MAX),
            links_remaining: u32::try_from(links_left).unwrap_or(u32::MAX),
            held_removed: u32::try_from(writes.held.len()).unwrap_or(u32::MAX),
            jobs_removed: u32::try_from(writes.jobs.len() + writes.dead.len()).unwrap_or(u32::MAX),
            erased_at: Utc::now(),
            entity_id: entity_id.to_string(),
            facts_removed: u32::try_from(facts.len()).unwrap_or(u32::MAX),
            links_removed: u32::try_from(rels.len()).unwrap_or(u32::MAX),
            derived_removed: u32::try_from(derived.len()).unwrap_or(u32::MAX),
            digest: digest_ids(ids),
        };
        tracing::warn!(
            target: "memory",
            facts = proof.facts_removed,
            links = proof.links_removed,
            derived = proof.derived_removed,
            "store.erase_entity"
        );
        Ok(proof)
    }

    async fn delete_relationship(
        &self,
        ctx: &ScopeContext,
        relationship_id: &str,
    ) -> Result<DeletionProof, StoreError> {
        let Some(rel) = self.get_relationship(ctx, relationship_id).await? else {
            return Err(StoreError::NotFound);
        };
        owner_gate(ctx, &rel.owner.user)?;
        let mut tx = self.begin().await?;
        let ids = vec![relationship_id.to_string()];
        self.delete_links(&mut tx, &ctx.tenant, &[], &ids).await?;
        let mut st = self.stmt();
        let (p, p_t) = (st.text(relationship_id), st.text(&ctx.tenant));
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM mem_relationship WHERE id = {p} AND tenant = {p_t}"
        ));
        if super::count_of(&tx.fetch(st).await.map_err(map_sql_err)?)? > 0 {
            return Err(StoreError::PartialDelete {
                removed: 0,
                expected: 1,
            });
        }
        tx.commit().await.map_err(map_sql_err)?;
        Ok(DeletionProof {
            entities_removed: Vec::new(),
            sources_removed: Vec::new(),
            deleted_at: Utc::now(),
            record_id: relationship_id.to_string(),
            derived_removed: Vec::new(),
            embeddings_removed: 0,
            digest: digest_ids(ids.clone()),
            relationships_removed: ids,
        })
    }

    async fn backfill_entity_names(&self, tenant: &str, batch: u32) -> Result<u64, StoreError> {
        let mut st = self.stmt();
        let cols = entity_cols(self.dialect);
        let p_t = st.text(tenant);
        let p_b = st.int(i64::from(batch.max(1)));
        let collate = id_collation(self.dialect);
        st.sql(&format!(
            "SELECT {cols} FROM mem_entity e WHERE e.tenant = {p_t} AND NOT EXISTS \
             (SELECT 1 FROM mem_entity_name n WHERE n.entity_id = e.id) \
             ORDER BY e.id{collate} LIMIT {p_b}"
        ));
        let entities: Vec<Entity> = self
            .fetch(st)
            .await?
            .iter()
            .map(row_to_entity)
            .collect::<Result<_, _>>()?;
        if !entities.is_empty() {
            let mut tx = self.begin().await?;
            for e in &entities {
                self.write_entity_names(&mut tx, e).await?;
            }
            tx.commit().await.map_err(map_sql_err)?;
            return Ok(entities.len() as u64);
        }
        // As in the SurrealDB store: written only while unset.
        let mut st = self.stmt();
        let (p_t, p_now) = (st.text(tenant), st.ts(Utc::now()));
        st.sql(&format!(
            "INSERT INTO mem_maintenance_state (tenant, names_backfilled_at) VALUES ({p_t}, {p_now}) \
             ON CONFLICT (tenant) DO UPDATE SET names_backfilled_at = \
             COALESCE(mem_maintenance_state.names_backfilled_at, excluded.names_backfilled_at)"
        ));
        self.execute(st).await?;
        self.backfilled
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(tenant.to_string());
        Ok(0)
    }
}
