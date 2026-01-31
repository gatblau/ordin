//! `GraphStore` over SurrealDB (memory-store amendment January 2026, ADR-021), and the graph steps the
//! store's existing operations gain: the closing rule of `end_validity` / `supersede` (11′, 12′), the
//! orphaned-link removal of `hard_delete` (13′), the folding of `merge_entities` (19d′), and the graph
//! and link runs of `recall` (9″).
//!
//! Every caller value is bound; the statement text is built from constants only (sql-safety layer).
//! Relationship and fact ids are stored in `relationship_fact` as the domain's `table:key` strings, as
//! `fact.entities` stores entity ids.

use std::collections::{BTreeMap, HashMap, HashSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value as Json;
use surrealdb::engine::any::Any;
use surrealdb::types::Value;

use super::{
    bind_valid_at, convert, fact_read_filter, id_thing, is_maintenance_scope, novis_filter_clause,
    validity_clause, vector_read_filter, Store,
};
use crate::store::graph::{
    check_limit, digest_ids, link_row_key, normalised_search, pick_hop, EntityCursor, HopEdge,
    ENTITY_DETAIL_MAX, ENTITY_PAGE_MAX, FACT_VERSIONS_MAX,
};
use crate::store::surreal::migrate::map_db_err;
use crate::store::{owner_gate, owner_view};
use crate::types::api::DeletionProof;
use crate::types::domain::{Entity, Fact, Relationship};
use crate::types::ports::{
    EntityCount, EntityDetail, EntityErasureProof, EntityPage, EntitySummary, GraphStore, LinkRole,
    MemoryStore, Neighbour, StoreError,
};
use crate::types::scope::ScopeContext;

type Query<'q> = surrealdb::method::Query<'q, Any>;

/// Bind the read-filter parameters of `ctx`.
fn bind_ctx<'q>(q: Query<'q>, ctx: &ScopeContext) -> Query<'q> {
    q.bind(("ctenant", ctx.tenant.clone()))
        .bind(("cuser", ctx.user.clone()))
        .bind(("cteams", ctx.teams.clone()))
        .bind(("cnow", Utc::now()))
}

fn take_rows(
    resp: &mut surrealdb::IndexedResults,
    slot: usize,
    what: &str,
) -> Result<Vec<Json>, StoreError> {
    resp.take(slot)
        .map_err(|e| StoreError::Internal(format!("{what}: {e}")))
}

fn str_field(row: &Json, field: &str) -> Option<String> {
    row.get(field).and_then(|v| v.as_str()).map(str::to_string)
}

fn things(ids: &[String]) -> Result<Vec<Value>, StoreError> {
    ids.iter().map(|i| id_thing(i)).collect()
}

/// `entity.private_names_norm`: the normalised private aliases not also a public name.
pub(super) fn private_names_norm(e: &Entity) -> Vec<String> {
    let public = crate::text::names_norm(&e.canonical_name, &e.public_aliases());
    crate::text::names_norm(&e.canonical_name, &e.private_aliases)
        .into_iter()
        .filter(|n| !public.contains(n))
        .collect()
}

impl Store {
    /// Facts naming `entity` outside `facts` and relationships touching it outside `rels`, whoever owns
    /// them.
    async fn left_on_entity(
        &self,
        tenant: &str,
        entity: &str,
        facts: &[String],
        rels: &[String],
    ) -> Result<(u32, u32), StoreError> {
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT meta::id(id) AS k FROM fact WITH INDEX fact_ent WHERE $entity IN entities; \
                 SELECT meta::id(id) AS k FROM relationship \
                   WHERE from_ent = $entity OR to_ent = $entity",
            )?)
            .bind(("entity", entity.to_string()))
            .await
            .map_err(map_db_err)?;
        let gone: HashSet<&String> = facts.iter().collect();
        let rels_gone: HashSet<&String> = rels.iter().collect();
        let facts_left: HashSet<String> = take_rows(&mut resp, 1, "facts naming the entity")?
            .iter()
            .filter_map(|r| str_field(r, "k").map(|k| format!("fact:{k}")))
            .filter(|id| !gone.contains(id))
            .collect();
        let links_left: HashSet<String> = take_rows(&mut resp, 2, "links touching the entity")?
            .iter()
            .filter_map(|r| str_field(r, "k").map(|k| format!("relationship:{k}")))
            .filter(|id| !rels_gone.contains(id))
            .collect();
        Ok((
            u32::try_from(facts_left.len()).unwrap_or(u32::MAX),
            u32::try_from(links_left.len()).unwrap_or(u32::MAX),
        ))
    }

    /// Those of `entities` the caller may see, as it sees them: its own, and those a fact it can read
    /// names. A link grants nothing and cannot reveal what the fact filter hides (SA-GRAPH-05).
    // spec: components/memory-store#SA-GRAPH-05
    // As `entity_read_filter` of the SQL storage, applied after the read because a
    // SurrealQL subquery per row costs more than one query over the rows.
    pub(super) async fn visible_entities(
        &self,
        ctx: &ScopeContext,
        entities: Vec<Entity>,
    ) -> Result<Vec<Entity>, StoreError> {
        if is_maintenance_scope(ctx) {
            return Ok(entities);
        }
        let others: Vec<String> = entities
            .iter()
            .filter(|e| !owner_view(ctx, &e.owner.user))
            .map(|e| e.id.clone())
            .collect();
        let mut named: HashSet<String> = HashSet::new();
        if !others.is_empty() {
            let mut resp = bind_ctx(
                self.db.query(self.scoped(
                    &ctx.tenant,
                    format!(
                        "SELECT entities FROM fact WHERE entities CONTAINSANY $ids AND ({})",
                        fact_read_filter(ctx)
                    ),
                )?),
                ctx,
            )
            .bind(("ids", others))
            .await
            .map_err(map_db_err)?;
            for r in take_rows(&mut resp, 1, "entities of readable facts")? {
                let ents: Vec<String> = r
                    .get("entities")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                named.extend(ents);
            }
        }
        Ok(entities
            .into_iter()
            .filter(|e| owner_view(ctx, &e.owner.user) || named.contains(&e.id))
            .map(|e| {
                let own = owner_view(ctx, &e.owner.user);
                e.as_seen_by(own)
            })
            .collect())
    }

    /// Those of `rels` the caller may read: its own, one a fact it can read records (either role), or a
    /// person's link of its team not held to its owner. As `rel_read_filter` of the SQL storage.
    pub(super) async fn visible_relationships(
        &self,
        ctx: &ScopeContext,
        rels: Vec<Relationship>,
    ) -> Result<Vec<Relationship>, StoreError> {
        if is_maintenance_scope(ctx) {
            return Ok(rels);
        }
        let plain = |r: &Relationship| {
            owner_view(ctx, &r.owner.user)
                || (r.replaces.is_some()
                    && !r.owner_only
                    && r.owner.team.as_ref().is_some_and(|t| ctx.teams.contains(t)))
        };
        let others: Vec<String> = rels
            .iter()
            .filter(|r| !plain(r))
            .map(|r| r.id.clone())
            .collect();
        let recorded = self.relationships_with_readable_fact(ctx, others).await?;
        Ok(rels
            .into_iter()
            .filter(|r| plain(r) || recorded.contains(&r.id))
            .collect())
    }

    /// Of `ids`, the relationships with a fact row (either role) of a fact the caller can read.
    async fn relationships_with_readable_fact(
        &self,
        ctx: &ScopeContext,
        ids: Vec<String>,
    ) -> Result<HashSet<String>, StoreError> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT relationship, fact FROM relationship_fact WHERE relationship IN $r",
            )?)
            .bind(("r", ids))
            .await
            .map_err(map_db_err)?;
        let rows = take_rows(&mut resp, 1, "link fact rows")?;
        let facts: Vec<String> = rows
            .iter()
            .filter_map(|r| str_field(r, "fact"))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if facts.is_empty() {
            return Ok(HashSet::new());
        }
        let mut resp = bind_ctx(
            self.db.query(self.scoped(
                &ctx.tenant,
                format!(
                    "SELECT meta::id(id) AS k FROM fact WHERE id IN $f AND ({})",
                    fact_read_filter(ctx)
                ),
            )?),
            ctx,
        )
        .bind(("f", things(&facts)?))
        .await
        .map_err(map_db_err)?;
        let readable: HashSet<String> = take_rows(&mut resp, 1, "readable link facts")?
            .iter()
            .filter_map(|r| str_field(r, "k").map(|k| format!("fact:{k}")))
            .collect();
        Ok(rows
            .iter()
            .filter_map(|r| {
                let rel = str_field(r, "relationship")?;
                readable.contains(&str_field(r, "fact")?).then_some(rel)
            })
            .collect())
    }

    /// Whether a fact naming `entity` compares to private by `op`, `=` or `!=` (`merge_entities`).
    pub(super) async fn entity_has_fact(
        &self,
        tenant: &str,
        entity: &str,
        op: &'static str,
    ) -> Result<bool, StoreError> {
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                format!(
                    "SELECT VALUE id FROM fact WITH INDEX fact_ent WHERE $entity IN entities \
                     AND visibility {op} 'user-private' LIMIT 1"
                ),
            )?)
            .bind(("entity", entity.to_string()))
            .await
            .map_err(map_db_err)?;
        Ok(!take_rows(&mut resp, 1, "entity shared")?.is_empty())
    }

    /// The caller's writes still on their way that name `entity`: queued and failed jobs, held records.
    // Only the caller's, as in the SQL storage.
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
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT meta::id(id) AS k, payload FROM work_job \
                   WHERE scope.tenant = $t AND scope.user = $u; \
                 SELECT meta::id(id) AS k, payload FROM dead_letter \
                   WHERE scope.tenant = $t AND scope.user = $u; \
                 SELECT meta::id(id) AS k, content, entities, entities_asserted, links_asserted \
                   FROM quarantine WHERE owner.tenant = $t AND owner.user = $u",
            )?)
            .bind(("t", ctx.tenant.clone()))
            .bind(("u", ctx.user.clone()))
            .await
            .map_err(map_db_err)?;
        let mut out = crate::store::graph::PendingWrites::default();
        for r in take_rows(&mut resp, 1, "jobs of the caller")? {
            let payload = r.get("payload").cloned().unwrap_or(Json::Null);
            if let (true, Some(k)) = (
                crate::store::payload_names(&payload, &names),
                str_field(&r, "k"),
            ) {
                out.jobs.push(format!("work_job:{k}"));
            }
        }
        for r in take_rows(&mut resp, 2, "dead letters of the caller")? {
            let payload = r.get("payload").cloned().unwrap_or(Json::Null);
            if let (true, Some(k)) = (
                crate::store::payload_names(&payload, &names),
                str_field(&r, "k"),
            ) {
                out.dead.push(format!("dead_letter:{k}"));
            }
        }
        for r in take_rows(&mut resp, 3, "held writes of the caller")? {
            let field = |n: &str| r.get(n).cloned().unwrap_or(Json::Null);
            let names_it = field("entities")
                .as_array()
                .is_some_and(|a| a.iter().any(|e| e.as_str() == Some(&entity.id)));
            let held = crate::store::names_in_write(
                &field("content"),
                &field("entities_asserted"),
                &field("links_asserted"),
            );
            if let (true, Some(k)) = (names_it || !held.is_disjoint(&names), str_field(&r, "k")) {
                out.held.push(format!("quarantine:{k}"));
            }
        }
        Ok(out)
    }

    /// Whether `tenant`'s entity names are backfilled (SA-GRAPH-12); cached once set.
    pub(super) async fn names_backfilled(&self, tenant: &str) -> Result<bool, StoreError> {
        if self
            .backfilled
            .read()
            .map_err(|_| StoreError::Internal("backfill cache poisoned".into()))?
            .contains(tenant)
        {
            return Ok(true);
        }
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT names_backfilled_at FROM maintenance_state WHERE tenant = $t LIMIT 1",
            )?)
            .bind(("t", tenant.to_string()))
            .await
            .map_err(map_db_err)?;
        let rows = take_rows(&mut resp, 1, "names_backfilled_at")?;
        let set = rows
            .first()
            .and_then(|r| r.get("names_backfilled_at"))
            .is_some_and(|v| !v.is_null());
        if set {
            self.backfilled
                .write()
                .map_err(|_| StoreError::Internal("backfill cache poisoned".into()))?
                .insert(tenant.to_string());
        }
        Ok(set)
    }

    /// Steps 11′ / 12′: the live relationships `fact_id` asserted, whose owner is the fact's owner,
    /// that no other currently valid fact of that owner asserts — the ones to close when `fact_id`
    /// ends. A teammate's fact neither ends a link nor keeps one live.
    // As `close_links` of the SQL storage; the rule follows the fact, under every scope.
    pub(super) async fn links_closed_by(
        &self,
        tenant: &str,
        fact_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT VALUE relationship FROM relationship_fact WHERE fact = $f AND role = 'asserted'",
            )?)
            .bind(("f", fact_id.to_string()))
            .await
            .map_err(map_db_err)?;
        let rels: Vec<String> = resp
            .take::<Vec<Json>>(1)
            .map_err(|e| StoreError::Internal(format!("links of fact: {e}")))?
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        if rels.is_empty() {
            return Ok(Vec::new());
        }
        // The other asserting facts of those relationships, and which of them are still valid.
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT relationship, fact FROM relationship_fact \
                 WHERE relationship IN $rels AND role = 'asserted' AND fact != $f; \
                 SELECT meta::id(id) AS k, owner.user AS who FROM relationship \
                   WHERE id IN $rthings AND valid_to IS NONE; \
                 SELECT owner.user AS who FROM $fthing",
            )?)
            .bind(("rels", rels.clone()))
            .bind(("f", fact_id.to_string()))
            .bind(("rthings", things(&rels)?))
            .bind(("fthing", id_thing(fact_id)?))
            .await
            .map_err(map_db_err)?;
        let others = take_rows(&mut resp, 1, "other asserting facts")?;
        let live_rows = take_rows(&mut resp, 2, "live relationships")?;
        let fact_owner = take_rows(&mut resp, 3, "the fact's owner")?
            .first()
            .and_then(|r| str_field(r, "who"));
        let live: HashSet<String> = live_rows
            .iter()
            .filter(|r| str_field(r, "who") == fact_owner)
            .filter_map(|r| str_field(r, "k").map(|k| format!("relationship:{k}")))
            .collect();
        let other_facts: Vec<String> = others
            .iter()
            .filter_map(|r| str_field(r, "fact"))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let valid: HashSet<String> = if other_facts.is_empty() {
            HashSet::new()
        } else {
            let mut resp = self
                .db
                .query(self.scoped(
                    tenant,
                    "SELECT meta::id(id) AS k, owner.user AS who FROM fact \
                     WHERE id IN $f AND valid_to IS NONE",
                )?)
                .bind(("f", things(&other_facts)?))
                .await
                .map_err(map_db_err)?;
            take_rows(&mut resp, 1, "valid asserting facts")?
                .iter()
                .filter(|r| str_field(r, "who") == fact_owner)
                .filter_map(|r| str_field(r, "k").map(|k| format!("fact:{k}")))
                .collect()
        };
        let mut kept: HashSet<String> = HashSet::new();
        for r in &others {
            if let (Some(rel), Some(f)) = (str_field(r, "relationship"), str_field(r, "fact")) {
                if valid.contains(&f) {
                    kept.insert(rel);
                }
            }
        }
        Ok(rels
            .into_iter()
            .filter(|r| live.contains(r) && !kept.contains(r))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect())
    }

    /// The names only `team`'s erased folder stated, or any of its folders when `collection` is `None`,
    /// leave the tenant's entities that outlive the erasure.
    pub(super) async fn drop_folder_aliases(
        &self,
        tenant: &str,
        team: &str,
        collection: Option<&str>,
    ) -> Result<(), StoreError> {
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT * FROM entity WHERE owner.tenant = $t AND $team IN alias_folders.*.team",
            )?)
            .bind(("t", tenant.to_string()))
            .bind(("team", team.to_string()))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("entities of the folder: {e}")))?;
        for row in rows {
            let mut entity = convert::row_to_entity(row)?;
            if crate::store::graph::drop_alias_folder(&mut entity, team, collection) {
                self.put_entity(&entity).await?;
            }
        }
        Ok(())
    }

    /// Remove the jobs that wrote `facts`, their parked copies and their links. A worker that still
    /// holds such a job finds it gone and takes its write back.
    pub(super) async fn drop_jobs_of(
        &self,
        tenant: &str,
        facts: &[String],
    ) -> Result<(), StoreError> {
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT VALUE job FROM job_fact WHERE scope.tenant = $t AND fact IN $facts",
            )?)
            .bind(("t", tenant.to_string()))
            .bind(("facts", facts.to_vec()))
            .await
            .map_err(map_db_err)?;
        let jobs: Vec<String> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("jobs of the facts: {e}")))?;
        if jobs.is_empty() {
            return Ok(());
        }
        let things = jobs
            .iter()
            .map(|j| id_thing(j))
            .collect::<Result<Vec<_>, _>>()?;
        self.db
            .query(self.scoped(
                tenant,
                "BEGIN; \
                 DELETE $things; \
                 DELETE dead_letter WHERE scope.tenant = $t AND original_job_id IN $jobs; \
                 DELETE job_fact WHERE scope.tenant = $t AND job IN $jobs; \
                 COMMIT;",
            )?)
            .bind(("t", tenant.to_string()))
            .bind(("things", things))
            .bind(("jobs", jobs))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    /// Step 13‴: the entities and sources `facts` name, owned by those facts' owners, that nothing
    /// outside `removed` and `rels` uses, and no held record names. As `unused_after` of the SQL
    /// storage, where the reading is stated; here it reads before the delete's own transaction.
    /// Step 13⁗ (SA-TRIPLE-10): a relationship of the removed facts' owner with no `asserted` row (a
    /// person's correction, an imported link) does not keep an entity; the third list holds those of
    /// them that touch an entity that goes, which go with it.
    // spec: phase-1-analysis#SA-DELETE-02
    // spec: phase-1-analysis#SA-TRIPLE-10
    pub(super) async fn unused_after(
        &self,
        ctx: &ScopeContext,
        facts: &[Fact],
        removed: &[String],
        rels: &[String],
    ) -> Result<(Vec<String>, Vec<String>, Vec<String>), StoreError> {
        let (owners, entities, sources) = crate::store::graph::delete_candidates(facts);
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT meta::id(id) AS k, entities, source_id FROM fact \
                   WHERE entities CONTAINSANY $ents; \
                 SELECT meta::id(id) AS k, source_id FROM fact WHERE source_id IN $srcs; \
                 SELECT meta::id(id) AS k, from_ent, to_ent, source_id, owner.user AS who \
                   FROM relationship \
                   WHERE from_ent IN $ents OR to_ent IN $ents OR source_id IN $srcs; \
                 SELECT entities, source_id FROM quarantine \
                   WHERE entities CONTAINSANY $ents OR source_id IN $srcs",
            )?)
            .bind(("ents", entities.clone()))
            .bind(("srcs", sources.clone()))
            .await
            .map_err(map_db_err)?;
        let gone: HashSet<&String> = removed.iter().collect();
        let rels_gone: HashSet<&String> = rels.iter().collect();
        let mut used: HashSet<String> = HashSet::new();
        let note = |used: &mut HashSet<String>, r: &Json, fields: &[&str]| {
            for f in fields {
                match r.get(*f) {
                    Some(Json::String(s)) => {
                        used.insert(s.clone());
                    }
                    Some(Json::Array(a)) => {
                        used.extend(a.iter().filter_map(|e| e.as_str().map(str::to_string)));
                    }
                    _ => {}
                }
            }
        };
        for slot in [1, 2] {
            for r in take_rows(&mut resp, slot, "facts using the candidates")? {
                let id = str_field(&r, "k").map(|k| format!("fact:{k}"));
                if !id.is_some_and(|id| gone.contains(&id)) {
                    note(&mut used, &r, &["entities", "source_id"]);
                }
            }
        }
        let rel_rows = take_rows(&mut resp, 3, "relationships using the candidates")?;
        for r in take_rows(&mut resp, 4, "held writes using the candidates")? {
            note(&mut used, &r, &["entities", "source_id"]);
        }
        let rel_ids: Vec<String> = rel_rows
            .iter()
            .filter_map(|r| str_field(r, "k").map(|k| format!("relationship:{k}")))
            .collect();
        let asserted: HashSet<String> = if rel_ids.is_empty() {
            HashSet::new()
        } else {
            let mut resp = self
                .db
                .query(self.scoped(
                    &ctx.tenant,
                    "SELECT VALUE relationship FROM relationship_fact \
                     WHERE relationship IN $r AND role = 'asserted'",
                )?)
                .bind(("r", rel_ids))
                .await
                .map_err(map_db_err)?;
            take_rows(&mut resp, 1, "asserted links")?
                .into_iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        };
        // (id, from, to) of each person-made link of the owners: it keeps no entity of theirs.
        let mut person_links: Vec<(String, String, String)> = Vec::new();
        for r in &rel_rows {
            let Some(id) = str_field(r, "k").map(|k| format!("relationship:{k}")) else {
                continue;
            };
            if rels_gone.contains(&id) {
                continue;
            }
            let ours = str_field(r, "who").is_some_and(|w| owners.contains(&w));
            if ours && !asserted.contains(&id) {
                note(&mut used, r, &["source_id"]);
                person_links.push((
                    id,
                    str_field(r, "from_ent").unwrap_or_default(),
                    str_field(r, "to_ent").unwrap_or_default(),
                ));
            } else {
                note(&mut used, r, &["from_ent", "to_ent", "source_id"]);
            }
        }
        let mut unused_entities = Vec::new();
        for id in entities.into_iter().filter(|e| !used.contains(e)) {
            if self
                .get_entity(ctx, &id)
                .await?
                .is_some_and(|e| owners.contains(&e.owner.user))
            {
                unused_entities.push(id);
            }
        }
        let going: HashSet<&String> = unused_entities.iter().collect();
        let links_going: Vec<String> = person_links
            .into_iter()
            .filter(|(_, from, to)| going.contains(from) || going.contains(to))
            .map(|(id, _, _)| id)
            .collect();
        let mut unused_sources = Vec::new();
        for id in sources.into_iter().filter(|s| !used.contains(s)) {
            if self
                .get_source(ctx, &id)
                .await?
                .is_some_and(|s| owners.contains(&s.owner.user))
            {
                unused_sources.push(id);
            }
        }
        Ok((unused_entities, unused_sources, links_going))
    }

    /// Step 13′: of the relationships removed facts of their own owner have rows for, those left with
    /// no asserting row from a remaining fact and no `replaces` — deleted with the facts.
    // As `orphaned_by` of the SQL storage. Runs while the removed facts still exist.
    pub(super) async fn orphaned_by(
        &self,
        tenant: &str,
        removed_facts: &[String],
        also_removed: &HashSet<String>,
    ) -> Result<Vec<String>, StoreError> {
        if removed_facts.is_empty() {
            return Ok(Vec::new());
        }
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT relationship, fact FROM relationship_fact WHERE fact IN $f; \
                 SELECT meta::id(id) AS k, owner.user AS who FROM fact WHERE id IN $ft",
            )?)
            .bind(("f", removed_facts.to_vec()))
            .bind(("ft", things(removed_facts)?))
            .await
            .map_err(map_db_err)?;
        let rows = take_rows(&mut resp, 1, "orphan candidates")?;
        let fact_owner: HashMap<String, String> = take_rows(&mut resp, 2, "removed fact owners")?
            .iter()
            .filter_map(|r| Some((format!("fact:{}", str_field(r, "k")?), str_field(r, "who")?)))
            .collect();
        let cands: Vec<String> = rows
            .iter()
            .filter_map(|r| str_field(r, "relationship"))
            .filter(|r| !also_removed.contains(r))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if cands.is_empty() {
            return Ok(Vec::new());
        }
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT relationship, fact FROM relationship_fact \
                 WHERE relationship IN $c AND role = 'asserted' AND fact NOT IN $f; \
                 SELECT meta::id(id) AS k, replaces, owner.user AS who FROM relationship \
                   WHERE id IN $ct",
            )?)
            .bind(("c", cands.clone()))
            .bind(("f", removed_facts.to_vec()))
            .bind(("ct", things(&cands)?))
            .await
            .map_err(map_db_err)?;
        let remaining = take_rows(&mut resp, 1, "remaining assertions")?;
        let mut owner_of: HashMap<String, String> = HashMap::new();
        let mut person: HashSet<String> = HashSet::new();
        for r in take_rows(&mut resp, 2, "candidate links")? {
            let Some(id) = str_field(&r, "k").map(|k| format!("relationship:{k}")) else {
                continue;
            };
            if str_field(&r, "replaces").is_some() {
                person.insert(id.clone());
            }
            if let Some(who) = str_field(&r, "who") {
                owner_of.insert(id, who);
            }
        }
        let own_row: HashSet<String> = rows
            .iter()
            .filter_map(|r| {
                let rel = str_field(r, "relationship")?;
                let who = fact_owner.get(&str_field(r, "fact")?)?;
                (owner_of.get(&rel) == Some(who)).then_some(rel)
            })
            .collect();
        // Only a remaining fact of the link's owner keeps the link.
        let remaining_facts: Vec<String> = remaining
            .iter()
            .filter_map(|r| str_field(r, "fact"))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let remaining_owner: HashMap<String, String> = if remaining_facts.is_empty() {
            HashMap::new()
        } else {
            let mut resp = self
                .db
                .query(self.scoped(
                    tenant,
                    "SELECT meta::id(id) AS k, owner.user AS who FROM fact WHERE id IN $f",
                )?)
                .bind(("f", things(&remaining_facts)?))
                .await
                .map_err(map_db_err)?;
            take_rows(&mut resp, 1, "remaining fact owners")?
                .iter()
                .filter_map(|r| {
                    Some((format!("fact:{}", str_field(r, "k")?), str_field(r, "who")?))
                })
                .collect()
        };
        let still: HashSet<String> = remaining
            .iter()
            .filter_map(|r| {
                let rel = str_field(r, "relationship")?;
                let who = remaining_owner.get(&str_field(r, "fact")?)?;
                (owner_of.get(&rel) == Some(who)).then_some(rel)
            })
            .collect();
        let mut out: Vec<String> = cands
            .into_iter()
            .filter(|r| !still.contains(r) && !person.contains(r) && own_row.contains(r))
            .collect();
        out.sort();
        Ok(out)
    }

    /// One graph or link run of step 9″ over `collection`: read-filtered, valid facts naming any of
    /// `entities`, newest `ingested_at` first (ties by id), at most `stage1_k`, each with its entities.
    pub(super) async fn entity_run(
        &self,
        ctx: &ScopeContext,
        q: &crate::types::ports::StageOneQuery,
        entities: &[String],
        collection: Option<&str>,
    ) -> Result<Vec<(String, Vec<String>)>, StoreError> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }
        let k = i64::from(q.stage1_k.max(1));
        let (read, _) = vector_read_filter(self.profile);
        let validity = validity_clause(q.filters.valid_at);
        let coll = if collection.is_some() {
            " AND collection = $coll"
        } else {
            ""
        };
        // One indexed `$entity IN entities` probe per entity (served by `fact_ent`, RISK-018), merged
        // here: the hint is sound only on a single-element conjunct, which is what each probe is.
        let mut by_id: HashMap<String, (String, Vec<String>)> = HashMap::new();
        for e in entities {
            let sql = format!(
                "SELECT meta::id(id) AS fid, ingested_at, entities FROM fact WITH INDEX fact_ent \
                 WHERE $entity IN entities AND ({read}) AND {validity}{coll} \
                 ORDER BY ingested_at DESC LIMIT $k"
            );
            let mut query = bind_ctx(self.db.query(self.scoped(&ctx.tenant, sql)?), ctx)
                .bind(("entity", e.clone()))
                .bind(("k", k));
            if let Some(c) = collection {
                query = query.bind(("coll", c.to_string()));
            }
            query = bind_valid_at(query, q.filters.valid_at);
            let mut resp = query.await.map_err(map_db_err)?;
            for r in take_rows(&mut resp, 1, "graph run")? {
                let Some(fid) = str_field(&r, "fid") else {
                    continue;
                };
                let at = str_field(&r, "ingested_at").unwrap_or_default();
                let ents: Vec<String> = r
                    .get("entities")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                by_id.insert(fid, (at, ents));
            }
        }
        let mut all: Vec<(String, String, Vec<String>)> = by_id
            .into_iter()
            .map(|(id, (at, ents))| (id, at, ents))
            .collect();
        // RFC 3339 instants of one precision order as strings.
        all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        all.truncate(k as usize);
        Ok(all.into_iter().map(|(id, _, e)| (id, e)).collect())
    }

    /// Step 9″.7: the trust signal of each cited source, read-filtered; an absent source is missing
    /// from the map (the caller reads 1.0).
    pub(super) async fn source_trusts(
        &self,
        ctx: &ScopeContext,
        source_ids: &[String],
    ) -> Result<HashMap<String, f64>, StoreError> {
        if source_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut resp = bind_ctx(
            self.db.query(self.scoped(
                &ctx.tenant,
                // Every id comes from a fact the read filter admitted, and its source belongs to that
                // fact's owner: a reader outside the owner's team must see the same trust as the owner,
                // not the 1.0 an unreadable source would read as.
                "SELECT meta::id(id) AS k, trust_signal FROM source \
                 WHERE id IN $ids AND owner.tenant = $ctenant",
            )?),
            ctx,
        )
        .bind(("ids", things(source_ids)?))
        .await
        .map_err(map_db_err)?;
        Ok(take_rows(&mut resp, 1, "source trust")?
            .iter()
            .filter_map(|r| {
                Some((
                    format!("source:{}", str_field(r, "k")?),
                    r.get("trust_signal")?.as_f64()?,
                ))
            })
            .collect())
    }

    /// Relationships touching any of `entities`, read-filtered, of any validity unless `live`.
    async fn relationships_touching(
        &self,
        ctx: &ScopeContext,
        entities: &[String],
        live: bool,
    ) -> Result<Vec<Relationship>, StoreError> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }
        let validity = if live { " AND valid_to IS NONE" } else { "" };
        // Two statements so each endpoint column is served by its own index (`rel_from`, `rel_to`).
        let mut resp = bind_ctx(
            self.db.query(self.scoped(
                &ctx.tenant,
                format!(
                    "SELECT * FROM relationship WHERE from_ent IN $e \
                       AND owner.tenant = $ctenant{validity}; \
                     SELECT * FROM relationship WHERE to_ent IN $e \
                       AND owner.tenant = $ctenant{validity}"
                ),
            )?),
            ctx,
        )
        .bind(("e", entities.to_vec()))
        .await
        .map_err(map_db_err)?;
        let mut out: BTreeMap<String, Relationship> = BTreeMap::new();
        for slot in [1, 2] {
            for row in take_rows(&mut resp, slot, "relationships touching")? {
                let r = convert::row_to_relationship(row)?;
                out.insert(r.id.clone(), r);
            }
        }
        self.visible_relationships(ctx, out.into_values().collect())
            .await
    }

    /// Readable facts naming `entity`, any validity unless `live`.
    async fn facts_naming(
        &self,
        ctx: &ScopeContext,
        entity: &str,
        live: bool,
    ) -> Result<Vec<Fact>, StoreError> {
        let validity = if live { " AND valid_to IS NONE" } else { "" };
        let mut resp = bind_ctx(
            self.db.query(self.scoped(
                &ctx.tenant,
                format!(
                    "SELECT * FROM fact WITH INDEX fact_ent WHERE $entity IN entities \
                     AND ({}){validity} ORDER BY ingested_at DESC",
                    fact_read_filter(ctx)
                ),
            )?),
            ctx,
        )
        .bind(("entity", entity.to_string()))
        .await
        .map_err(map_db_err)?;
        take_rows(&mut resp, 1, "facts naming")?
            .into_iter()
            .map(convert::row_to_fact)
            .collect()
    }

    /// Write `names_norm` for one entity (step 7′ for rows written before `0008`).
    async fn write_names_norm(
        &self,
        tenant: &str,
        id: &str,
        names: Vec<String>,
    ) -> Result<(), StoreError> {
        self.db
            .query(self.scoped(tenant, "UPDATE $id SET names_norm = $n")?)
            .bind(("id", id_thing(id)?))
            .bind(("n", names))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }
}

#[async_trait]
impl GraphStore for Store {
    async fn find_live_relationship(
        &self,
        ctx: &ScopeContext,
        from: &str,
        kind: &str,
        to: &str,
    ) -> Result<Option<Relationship>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let team = match ctx.teams.first() {
            Some(t) => Value::String(t.clone()),
            None => Value::None,
        };
        let mut resp = bind_ctx(
            self.db.query(self.scoped(
                &ctx.tenant,
                "SELECT * FROM relationship WITH INDEX relationship_live \
                 WHERE owner.team = $team AND from_ent = $from AND kind = $kind \
                 AND to_ent = $to AND valid_to IS NONE AND owner.tenant = $ctenant",
            )?),
            ctx,
        )
        .bind(("team", team))
        .bind(("from", from.to_string()))
        .bind(("kind", kind.to_string()))
        .bind(("to", to.to_string()))
        .await
        .map_err(map_db_err)?;
        let rows = take_rows(&mut resp, 1, "find live relationship")?
            .into_iter()
            .map(convert::row_to_relationship)
            .collect::<Result<Vec<_>, _>>()?;
        // As the SQL storage: the caller's own row first, then the smallest id.
        Ok(self
            .visible_relationships(ctx, rows)
            .await?
            .into_iter()
            .min_by_key(|r| (r.owner.user != ctx.user, r.id.clone())))
    }

    async fn relationship_kinds(
        &self,
        ctx: &ScopeContext,
        limit: u32,
    ) -> Result<Vec<String>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let mut resp = bind_ctx(
            self.db.query(self.scoped(
                &ctx.tenant,
                "SELECT * FROM relationship WHERE owner.tenant = $ctenant",
            )?),
            ctx,
        )
        .await
        .map_err(map_db_err)?;
        let rels = take_rows(&mut resp, 1, "relationship kinds")?
            .into_iter()
            .map(convert::row_to_relationship)
            .collect::<Result<Vec<_>, _>>()?;
        let mut last: HashMap<String, DateTime<Utc>> = HashMap::new();
        for r in self.visible_relationships(ctx, rels).await? {
            let slot = last.entry(r.kind).or_insert(r.ingested_at);
            *slot = (*slot).max(r.ingested_at);
        }
        let mut rows: Vec<(String, DateTime<Utc>)> = last.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        rows.truncate(limit as usize);
        Ok(rows.into_iter().map(|(k, _)| k).collect())
    }

    async fn link_fact(
        &self,
        ctx: &ScopeContext,
        relationship_id: &str,
        fact_id: &str,
        role: LinkRole,
    ) -> Result<(), StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let key = link_row_key(relationship_id, fact_id, role.as_str());
        let mut rec = surrealdb::types::Object::new();
        rec.insert("relationship", Value::String(relationship_id.to_string()));
        rec.insert("fact", Value::String(fact_id.to_string()));
        rec.insert("role", Value::String(role.as_str().to_string()));
        rec.insert(
            "at",
            Value::Datetime(surrealdb::types::Datetime::from(Utc::now())),
        );
        self.db
            .query(self.scoped(&ctx.tenant, "UPSERT $id CONTENT $rec")?)
            .bind(("id", id_thing(&format!("relationship_fact:{key}"))?))
            .bind(("rec", Value::Object(rec)))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn link_is_shared(&self, ctx: &ScopeContext, id: &str) -> Result<bool, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT VALUE fact FROM relationship_fact WHERE relationship = $r; \
                 SELECT replaces, owner_only FROM $id",
            )?)
            .bind(("r", id.to_string()))
            .bind(("id", id_thing(id)?))
            .await
            .map_err(map_db_err)?;
        let facts: Vec<String> = take_rows(&mut resp, 1, "link fact rows")?
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let person = take_rows(&mut resp, 2, "link")?.first().is_some_and(|r| {
            str_field(r, "replaces").is_some()
                && r.get("owner_only").and_then(Json::as_bool) != Some(true)
        });
        if person || facts.is_empty() {
            return Ok(person);
        }
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT VALUE id FROM fact WHERE id IN $f AND visibility != 'user-private' LIMIT 1",
            )?)
            .bind(("f", things(&facts)?))
            .await
            .map_err(map_db_err)?;
        Ok(!take_rows(&mut resp, 1, "shared link facts")?.is_empty())
    }

    async fn links_of_fact(
        &self,
        ctx: &ScopeContext,
        fact_id: &str,
    ) -> Result<Vec<(Relationship, LinkRole)>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT relationship, role FROM relationship_fact WHERE fact = $f",
            )?)
            .bind(("f", fact_id.to_string()))
            .await
            .map_err(map_db_err)?;
        let rows = take_rows(&mut resp, 1, "links of fact")?;
        let mut roles: Vec<(String, LinkRole)> = rows
            .iter()
            .filter_map(|r| {
                Some((
                    str_field(r, "relationship")?,
                    LinkRole::parse(&str_field(r, "role")?)?,
                ))
            })
            .collect();
        roles.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.as_str().cmp(b.1.as_str())));
        let ids: Vec<String> = roles.iter().map(|(r, _)| r.clone()).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut resp = bind_ctx(
            self.db.query(self.scoped(
                &ctx.tenant,
                "SELECT * FROM relationship WHERE id IN $ids AND owner.tenant = $ctenant",
            )?),
            ctx,
        )
        .bind(("ids", things(&ids)?))
        .await
        .map_err(map_db_err)?;
        let rows = take_rows(&mut resp, 1, "linked relationships")?
            .into_iter()
            .map(convert::row_to_relationship)
            .collect::<Result<Vec<_>, _>>()?;
        let rels: HashMap<String, Relationship> = self
            .visible_relationships(ctx, rows)
            .await?
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
        Ok(roles
            .into_iter()
            .filter_map(|(id, role)| rels.get(&id).map(|r| (r.clone(), role)))
            .collect())
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
        self.ensure_provisioned(&ctx.tenant).await?;
        let coll = if collections.is_some() {
            " AND collection IN $colls"
        } else {
            ""
        };
        // Every match is read (a query has at most 64 phrases) and the order and the limit are applied
        // here, so which entities survive the limit is not left to the engine.
        // The caller's private aliases match its own entities alone; each name set has its index.
        let mut query = bind_ctx(
            self.db.query(self.scoped(
                &ctx.tenant,
                format!(
                    "SELECT * FROM entity WHERE names_norm CONTAINSANY $phrases \
                       AND owner.tenant = $ctenant{coll}; \
                     SELECT * FROM entity WHERE private_names_norm CONTAINSANY $phrases \
                       AND owner.tenant = $ctenant AND owner.user = $cuser{coll}"
                ),
            )?),
            ctx,
        )
        .bind(("phrases", phrases.to_vec()));
        if let Some(c) = collections {
            query = query.bind(("colls", c.to_vec()));
        }
        let mut resp = query.await.map_err(map_db_err)?;
        let wanted: HashSet<&String> = phrases.iter().collect();
        let mut best: BTreeMap<String, usize> = BTreeMap::new();
        let mut rows: BTreeMap<String, Entity> = BTreeMap::new();
        for slot in [1, 2] {
            for row in take_rows(&mut resp, slot, "match entities")? {
                let name_field = if slot == 1 {
                    "names_norm"
                } else {
                    "private_names_norm"
                };
                let names: Vec<String> = row
                    .get(name_field)
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let len = names
                    .iter()
                    .filter(|n| wanted.contains(n))
                    .map(|n| n.chars().count())
                    .max()
                    .unwrap_or(0);
                let e = convert::row_to_entity(row)?;
                let cur = best.entry(e.id.clone()).or_insert(0);
                *cur = (*cur).max(len);
                rows.insert(e.id.clone(), e);
            }
        }
        let mut scored: Vec<(usize, Entity)> = self
            .visible_entities(ctx, rows.into_values().collect())
            .await?
            .into_iter()
            .map(|e| (best.get(&e.id).copied().unwrap_or(0), e))
            .collect();
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
        self.ensure_provisioned(&ctx.tenant).await?;
        let mut seen: HashSet<String> = seeds.iter().cloned().collect();
        let mut frontier: HashSet<String> = seeds.iter().cloned().collect();
        let mut out: Vec<Neighbour> = Vec::new();
        let validity = validity_clause(valid_at);
        for hop in 1..=hops {
            if frontier.is_empty() {
                break;
            }
            let f: Vec<String> = frontier.iter().cloned().collect();
            let novis = novis_filter_clause();
            // SA-GRAPH-05 reads the owner here; a correction held to its owner stays with it.
            let held = "(owner_only != true OR owner.user = $cuser)";
            let mut query = bind_ctx(
                self.db.query(self.scoped(
                    &ctx.tenant,
                    format!(
                        "SELECT meta::id(id) AS k, from_ent, to_ent, confidence, replaces FROM relationship \
                         WHERE from_ent IN $f AND ({novis}) AND {held} AND {validity}; \
                         SELECT meta::id(id) AS k, from_ent, to_ent, confidence, replaces FROM relationship \
                         WHERE to_ent IN $f AND ({novis}) AND {held} AND {validity}"
                    ),
                )?),
                ctx,
            )
            .bind(("f", f));
            query = bind_valid_at(query, valid_at);
            let mut resp = query.await.map_err(map_db_err)?;
            let mut edges: BTreeMap<String, HopEdge> = BTreeMap::new();
            for slot in [1, 2] {
                for r in take_rows(&mut resp, slot, "neighbour edges")? {
                    let (Some(k), Some(from), Some(to)) = (
                        str_field(&r, "k"),
                        str_field(&r, "from_ent"),
                        str_field(&r, "to_ent"),
                    ) else {
                        continue;
                    };
                    let id = format!("relationship:{k}");
                    edges.insert(
                        id.clone(),
                        HopEdge {
                            id,
                            from,
                            to,
                            confidence: r.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            replaces: str_field(&r, "replaces"),
                        },
                    );
                }
            }
            if edges.is_empty() {
                break;
            }
            // SA-GRAPH-05: a relationship is followed through a readable fact that asserts it and is
            // valid at the same time, or — with no asserting fact — when it is a person's link.
            let rel_ids: Vec<String> = edges.keys().cloned().collect();
            let mut resp = self
                .db
                .query(self.scoped(
                    &ctx.tenant,
                    "SELECT relationship, fact FROM relationship_fact \
                     WHERE relationship IN $r AND role = 'asserted'",
                )?)
                .bind(("r", rel_ids))
                .await
                .map_err(map_db_err)?;
            let rows = take_rows(&mut resp, 1, "asserting rows")?;
            let mut has_asserted: HashSet<String> = HashSet::new();
            let mut facts: HashSet<String> = HashSet::new();
            for r in &rows {
                if let (Some(rel), Some(f)) = (str_field(r, "relationship"), str_field(r, "fact")) {
                    has_asserted.insert(rel);
                    facts.insert(f);
                }
            }
            let readable: HashSet<String> = if facts.is_empty() {
                HashSet::new()
            } else {
                let fv: Vec<String> = facts.into_iter().collect();
                let mut query = bind_ctx(
                    self.db.query(self.scoped(
                        &ctx.tenant,
                        format!(
                            "SELECT meta::id(id) AS k FROM fact WHERE id IN $f AND ({}) AND {validity}",
                            fact_read_filter(ctx)
                        ),
                    )?),
                    ctx,
                )
                .bind(("f", things(&fv)?));
                query = bind_valid_at(query, valid_at);
                let mut resp = query.await.map_err(map_db_err)?;
                take_rows(&mut resp, 1, "readable asserting facts")?
                    .iter()
                    .filter_map(|r| str_field(r, "k").map(|k| format!("fact:{k}")))
                    .collect()
            };
            let asserted_readable: HashSet<String> = rows
                .iter()
                .filter_map(|r| {
                    let rel = str_field(r, "relationship")?;
                    readable.contains(&str_field(r, "fact")?).then_some(rel)
                })
                .collect();
            let picked = pick_hop(
                &frontier,
                edges.into_values().collect(),
                &has_asserted,
                &asserted_readable,
                &seen,
                fanout,
            );
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
        self.ensure_provisioned(&ctx.tenant).await?;
        // The normalised search contains no newline (whitespace is collapsed), so a match inside the
        // joined names is a match inside one name.
        let filter = if search.is_some() {
            " AND (string::contains(array::join(names_norm ?? [], '\\n'), $s) \
             OR (owner.user = $cuser \
                 AND string::contains(array::join(private_names_norm ?? [], '\\n'), $s)))"
        } else {
            ""
        };
        // A page of rows in key order, kept to what the caller may see, until `limit + 1` remain or the
        // tenant's entities end.
        let mut key = after;
        let mut entities: Vec<Entity> = Vec::new();
        loop {
            let (an, ak) = match &key {
                Some(c) => (
                    Value::String(c.n.clone()),
                    Value::String(super::record_key(&c.id)?),
                ),
                None => (Value::None, Value::None),
            };
            let keyset = if key.is_some() {
                " AND (canonical_name > $an OR (canonical_name = $an AND meta::id(id) > $ak))"
            } else {
                ""
            };
            // The caller's private names join the search on its own entities only.
            let mut resp = bind_ctx(
                self.db.query(self.scoped(
                    &ctx.tenant,
                    format!(
                        "SELECT *, meta::id(id) AS ekey FROM entity \
                         WHERE owner.tenant = $ctenant{keyset}{filter} \
                         ORDER BY canonical_name ASC, ekey ASC LIMIT $lim"
                    ),
                )?),
                ctx,
            )
            .bind(("an", an))
            .bind(("ak", ak))
            .bind(("s", search.clone().unwrap_or_default()))
            .bind(("lim", i64::from(limit) + 1))
            .await
            .map_err(map_db_err)?;
            let rows: Vec<Entity> = take_rows(&mut resp, 1, "list entities")?
                .into_iter()
                .map(|mut r| {
                    if let Some(o) = r.as_object_mut() {
                        o.remove("ekey");
                    }
                    convert::row_to_entity(r)
                })
                .collect::<Result<_, _>>()?;
            let exhausted = rows.len() <= limit as usize;
            key = rows.last().map(|e| EntityCursor {
                n: e.canonical_name.clone(),
                id: e.id.clone(),
            });
            entities.extend(self.visible_entities(ctx, rows).await?);
            if exhausted || entities.len() > limit as usize {
                break;
            }
        }
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
        // Two grouped counts over the page's ids.
        let ids: Vec<String> = entities.iter().map(|e| e.id.clone()).collect();
        let mut fact_count: HashMap<String, u32> = HashMap::new();
        if !ids.is_empty() {
            let mut resp = bind_ctx(
                self.db.query(self.scoped(
                    &ctx.tenant,
                    format!(
                        "SELECT entities FROM fact WHERE entities CONTAINSANY $ids AND ({}) \
                         AND valid_to IS NONE",
                        fact_read_filter(ctx)
                    ),
                )?),
                ctx,
            )
            .bind(("ids", ids.clone()))
            .await
            .map_err(map_db_err)?;
            let page: HashSet<&String> = ids.iter().collect();
            for r in take_rows(&mut resp, 1, "entity fact counts")? {
                let ents: Vec<String> = r
                    .get("entities")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                for e in ents.into_iter().collect::<HashSet<_>>() {
                    if page.contains(&e) {
                        *fact_count.entry(e).or_default() += 1;
                    }
                }
            }
        }
        let mut link_count: HashMap<String, u32> = HashMap::new();
        for r in self.relationships_touching(ctx, &ids, true).await? {
            *link_count.entry(r.from.clone()).or_default() += 1;
            if r.to != r.from {
                *link_count.entry(r.to.clone()).or_default() += 1;
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
        let mut facts = self.facts_naming(ctx, entity_id, true).await?;
        facts.truncate(limit as usize);
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
            let mut resp = bind_ctx(
                self.db.query(self.scoped(
                    &ctx.tenant,
                    "SELECT * FROM entity WHERE id IN $ids AND owner.tenant = $ctenant",
                )?),
                ctx,
            )
            .bind(("ids", things(&ends)?))
            .await
            .map_err(map_db_err)?;
            let rows = take_rows(&mut resp, 1, "endpoint names")?
                .into_iter()
                .map(convert::row_to_entity)
                .collect::<Result<Vec<_>, _>>()?;
            for e in self.visible_entities(ctx, rows).await? {
                names.insert(e.id, e.canonical_name);
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
        self.db
            .query(self.scoped(
                &ctx.tenant,
                "UPDATE $id SET memory_class = 'semantic', salience = math::max([salience, $floor])",
            )?)
            .bind(("id", id_thing(fact_id)?))
            .bind(("floor", salience_floor))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
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
        let facts = self.facts_naming(ctx, entity_id, false).await?.len();
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
            .facts_naming(ctx, entity_id, false)
            .await?
            .into_iter()
            .filter(|f| owner_view(ctx, &f.owner.user))
            .map(|f| f.id)
            .collect();
        // The consolidated facts derived from those.
        let derived: Vec<String> = if facts.is_empty() {
            Vec::new()
        } else {
            let mut resp = bind_ctx(
                self.db.query(self.scoped(
                    &ctx.tenant,
                    format!(
                        "SELECT meta::id(id) AS k, owner.user AS who FROM fact \
                         WHERE derived_from CONTAINSANY $f AND ({})",
                        fact_read_filter(ctx)
                    ),
                )?),
                ctx,
            )
            .bind(("f", facts.clone()))
            .await
            .map_err(map_db_err)?;
            let named: HashSet<&String> = facts.iter().collect();
            take_rows(&mut resp, 1, "derived facts")?
                .iter()
                .filter(|r| str_field(r, "who").is_some_and(|who| owner_view(ctx, &who)))
                .filter_map(|r| str_field(r, "k").map(|k| format!("fact:{k}")))
                .filter(|id| !named.contains(id))
                .collect()
        };
        let touching: Vec<String> = self
            .relationships_touching(ctx, &[entity_id.to_string()], false)
            .await?
            .into_iter()
            .filter(|r| owner_view(ctx, &r.owner.user))
            .map(|r| r.id)
            .collect();
        let mut removed_facts = facts.clone();
        removed_facts.extend(derived.iter().cloned());

        // The writes on their way first, committed apart, as `erase_collection`.
        let writes = self.writes_naming(ctx, &entity).await?;
        let pending: Vec<String> = writes.all().collect();
        self.db
            .query(self.scoped(&ctx.tenant, "DELETE $ids")?)
            .bind(("ids", things(&pending)?))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;

        let touching_set: HashSet<String> = touching.iter().cloned().collect();
        let orphans = self
            .orphaned_by(&ctx.tenant, &removed_facts, &touching_set)
            .await?;
        let mut rels = touching.clone();
        rels.extend(orphans.iter().cloned());

        // What other users hold is the outcome (SA-GRAPH-13), read past the read filter so a row the
        // caller cannot see still keeps the entity. As `erase_entity` of the SQL storage, where the
        // counts are described.
        // spec: phase-1-analysis#SA-GRAPH-13
        let (facts_left, links_left) = self
            .left_on_entity(&ctx.tenant, entity_id, &removed_facts, &rels)
            .await?;
        let entity_removed = facts_left == 0 && links_left == 0;
        let entity_things = if entity_removed {
            vec![id_thing(entity_id)?]
        } else {
            Vec::new()
        };

        let mut fact_things = things(&removed_facts)?;
        fact_things.extend(things(&rels)?);
        self.db
            .query(self.scoped(
                &ctx.tenant,
                // Step 20i′: the deleted facts' conflict rows go in the same transaction.
                // spec: components/memory-store#SA-TRIPLE-08
                "BEGIN; \
                 DELETE relationship_fact WHERE fact IN $fs OR relationship IN $rs; \
                 DELETE fact_conflict WHERE fact_a IN $fs OR fact_b IN $fs; \
                 DELETE $things; \
                 DELETE $entity; \
                 COMMIT;",
            )?)
            .bind(("fs", removed_facts.clone()))
            .bind(("rs", rels.clone()))
            .bind(("things", fact_things))
            .bind(("entity", entity_things))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;

        // A row of the removal set remaining is partial.
        let mut survived = 0u32;
        for f in &removed_facts {
            if self.get_fact(ctx, f).await?.is_some() {
                survived += 1;
            }
        }
        if survived > 0 {
            let expected = u32::try_from(removed_facts.len()).unwrap_or(u32::MAX);
            return Err(StoreError::PartialDelete {
                removed: expected.saturating_sub(survived),
                expected,
            });
        }

        self.sweep_summaries(ctx, &removed_facts).await?;
        let mut ids = removed_facts.clone();
        ids.extend(rels.iter().cloned());
        if entity_removed {
            ids.push(entity_id.to_string());
        }
        ids.extend(pending);
        let proof = EntityErasureProof {
            entity_removed,
            facts_remaining: facts_left,
            links_remaining: links_left,
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
        self.db
            .query(self.scoped(
                &ctx.tenant,
                "BEGIN; DELETE relationship_fact WHERE relationship = $r; DELETE $id; COMMIT;",
            )?)
            .bind(("r", relationship_id.to_string()))
            .bind(("id", id_thing(relationship_id)?))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        if self.get_relationship(ctx, relationship_id).await?.is_some() {
            return Err(StoreError::PartialDelete {
                removed: 0,
                expected: 1,
            });
        }
        Ok(DeletionProof {
            entities_removed: Vec::new(),
            sources_removed: Vec::new(),
            deleted_at: Utc::now(),
            record_id: relationship_id.to_string(),
            derived_removed: Vec::new(),
            embeddings_removed: 0,
            digest: digest_ids(vec![relationship_id.to_string()]),
            relationships_removed: vec![relationship_id.to_string()],
        })
    }

    async fn backfill_entity_names(&self, tenant: &str, batch: u32) -> Result<u64, StoreError> {
        self.ensure_provisioned(tenant).await?;
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT meta::id(id) AS k, canonical_name, aliases FROM entity \
                 WHERE owner.tenant = $t AND names_norm IS NONE LIMIT $b",
            )?)
            .bind(("t", tenant.to_string()))
            .bind(("b", i64::from(batch.max(1))))
            .await
            .map_err(map_db_err)?;
        let rows = take_rows(&mut resp, 1, "backfill names")?;
        let mut written = 0u64;
        for r in &rows {
            let Some(k) = str_field(r, "k") else {
                continue;
            };
            let canonical = str_field(r, "canonical_name").unwrap_or_default();
            let aliases: Vec<String> = r
                .get("aliases")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            self.write_names_norm(
                tenant,
                &format!("entity:{k}"),
                crate::text::names_norm(&canonical, &aliases),
            )
            .await?;
            written += 1;
        }
        if written == 0 {
            // The C7 cycle has no read of the field to decide whether to call (the maintenance-state
            // read returns two other fields). It calls every time, so the instant is written only
            // while it is unset: the first completed backfill is the one recorded.
            if !self.names_backfilled(tenant).await? {
                let key = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, tenant.as_bytes());
                self.db
                    .query(self.scoped(
                        tenant,
                        "UPSERT $id MERGE { tenant: $t, names_backfilled_at: $now }",
                    )?)
                    .bind(("id", id_thing(&format!("maintenance_state:{key}"))?))
                    .bind(("t", tenant.to_string()))
                    .bind(("now", surrealdb::types::Datetime::from(Utc::now())))
                    .await
                    .map_err(map_db_err)?
                    .check()
                    .map_err(map_db_err)?;
                self.backfilled
                    .write()
                    .map_err(|_| StoreError::Internal("backfill cache poisoned".into()))?
                    .insert(tenant.to_string());
            }
        }
        Ok(written)
    }
}
