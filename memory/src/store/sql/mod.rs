//! C1 — Memory Store, SQL implementation (feature `sql`, change M-2): the whole storage port
//! (`StoragePort`, ADR-101 item 1) over SQLite or PostgreSQL through `sqlx`, on a pool the host gives.
//!
//! The behaviour is the SurrealDB store's (`store::surreal`), which the memory component's own test
//! suite pins (ADR-101 item 4); what differs is where it comes from:
//!
//! * **Tenancy is a predicate, not a namespace.** Every tenant's rows share the `mem_` tables of
//!   *Data Model* §B, so every statement carries `tenant = $t` where SurrealDB selected a namespace.
//!   A write whose id already belongs to another tenant, or to another user of the tenant, is refused
//!   (`StoreError::ScopeForbidden`) rather than overwriting it.
//! * **Vector stage-1 is an exact cosine scan in Rust** over the embeddings the read filter admits —
//!   the filter-first strategy the SurrealDB store names `FilterFirstExact` — so a visible fact cannot
//!   be crowded out of the candidate window. The filter columns and vectors it reads are held in
//!   memory per tenant (`cache`), so a run scores without reading a row; the cache assumes this
//!   storage is the only writer of its tables.
//! * **Keyword stage-1** is FTS5 `bm25()` in SQLite and `ts_rank_cd` over the generated `content_tsv`
//!   column in PostgreSQL (§B), normalised by the batch maximum as the SurrealDB store does.
//! * **The schema is the host's to apply.** [`SqlStorage::migrations`] returns §B's steps for the
//!   host's migrator; the storage never runs DDL.
//!
//! All caller input is bound as a statement parameter (`exec::Stmt`); SQL text is built from
//! constants only (sql-safety layer rule).

mod cache;
mod ddl;
mod exec;
mod graph;
mod queue;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};

use crate::store::fusion::{self, ListKind};
use crate::store::{owner_gate, owner_view};
use crate::types::api::{DeletionProof, TenantErasureProof};
use crate::types::domain::{Entity, Fact, MemoryClass, Relationship, Source, Visibility};
use crate::types::ports::{
    AuditEntry, Candidate, CollectionErasure, IdempotencyRecord, IdempotencyStore,
    MaintenanceStateStore, MemoryStore, QuarantineStore, ScopeErasure, StageOneQuery, StoreError,
};
use crate::types::scope::{ScopeContext, ScopeRef};
use crate::write_pipeline::QuarantineRecord;

use cache::{LoadedFact, Plan, SearchCache, SearchFilter};
use exec::{json_col, map_sql_err, SqlRow, Stmt, Tx};

/// The SQL dialect a [`SqlStorage`] speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SqlDialect {
    Sqlite,
    Postgres,
}

/// The pool the host hands the storage. The dialect follows from the variant.
#[derive(Clone)]
pub enum SqlPool {
    Sqlite(sqlx::SqlitePool),
    Postgres(sqlx::PgPool),
}

impl SqlPool {
    fn dialect(&self) -> SqlDialect {
        match self {
            SqlPool::Sqlite(_) => SqlDialect::Sqlite,
            SqlPool::Postgres(_) => SqlDialect::Postgres,
        }
    }
}

/// The SQL memory store: the storage port over the `mem_` tables of *Data Model* §B.
pub struct SqlStorage {
    pool: SqlPool,
    dialect: SqlDialect,
    /// `MEMORY_EMBED_DIM` — every stored embedding is exactly this many `f32`s (§B).
    embed_dim: u32,
    /// `MEMORY_JOB_MAX_ATTEMPTS` — retry cap before dead-letter (SA-QUEUE-02).
    max_attempts: u32,
    /// `MEMORY_JOB_BACKOFF_BASE_MS` — backoff base for job retries.
    backoff_base_ms: u32,
    /// The per-scope in-memory search cache the exact vector scan and the collections step read
    /// (ordin change M-8 g; `cache`).
    embeddings: Arc<SearchCache>,
    /// Tenants whose entity names are known backfilled (SA-GRAPH-12); never unset once set.
    backfilled: RwLock<std::collections::HashSet<String>>,
    /// Whether `store.recall_graph_unavailable` has been logged in this process.
    graph_unavailable_logged: std::sync::atomic::AtomicBool,
}

/// `MEMORY_JOB_MAX_ATTEMPTS` default, as for a SurrealDB store opened without a `Config`.
const DEFAULT_JOB_MAX_ATTEMPTS: u32 = 5;
/// `MEMORY_JOB_BACKOFF_BASE_MS` default.
const DEFAULT_JOB_BACKOFF_BASE_MS: u32 = 2000;

/// What a `TenantErasureProof` and a refused namespace drop name as the namespace: every tenant shares
/// the `mem_`-prefixed tables of §B.
// SurrealDB gives each tenant a namespace; §B gives the SQL storage one set of shared tables and no
// namespace. So `ensure_tenant_namespace` has nothing to provision (the host applies
// §B), `drop_tenant_namespace` is always refused with `NamespaceNotExclusive` (the tables hold every
// tenant — the refusal ADR-018 prescribes for a shared namespace), `erase_tenant`'s proof names this
// value as its namespace, and `list_tenants` enumerates the distinct tenants of the tenant-carrying
// tables in place of the namespaces.
const SHARED_NAMESPACE: &str = "mem";

impl SqlStorage {
    /// Build the storage over `pool`, whose database the host has migrated with
    /// [`SqlStorage::migrations`]. Performs no I/O.
    pub fn new(pool: SqlPool, embed_dim: u32) -> Self {
        Self {
            dialect: pool.dialect(),
            pool,
            embed_dim,
            max_attempts: DEFAULT_JOB_MAX_ATTEMPTS,
            backoff_base_ms: DEFAULT_JOB_BACKOFF_BASE_MS,
            embeddings: Arc::default(),
            backfilled: RwLock::default(),
            graph_unavailable_logged: Default::default(),
        }
    }

    /// (version, SQL) pairs of *Data Model* §B, for the caller's migrator, in the order they apply.
    pub fn migrations(dialect: SqlDialect) -> &'static [(&'static str, &'static str)] {
        match dialect {
            SqlDialect::Sqlite => ddl::SQLITE_STEPS,
            SqlDialect::Postgres => ddl::POSTGRES_STEPS,
        }
    }

    /// Replace the work queue's retry policy (`MEMORY_JOB_MAX_ATTEMPTS`, `MEMORY_JOB_BACKOFF_BASE_MS`),
    /// as `Store::with_queue_retry` does for the SurrealDB store.
    // `new(pool, embed_dim)` carries no retry policy. This mirrors the SurrealDB store's builder so a
    // host can pass `job_max_attempts` and `job_backoff_base_ms` through; without it the crate defaults
    // (5 attempts, 2000 ms) apply.
    pub fn with_queue_retry(mut self, max_attempts: u32, backoff_base_ms: u32) -> Self {
        self.max_attempts = max_attempts;
        self.backoff_base_ms = backoff_base_ms;
        self
    }

    fn stmt(&self) -> Stmt {
        Stmt::new(self.dialect)
    }

    async fn fetch(&self, st: Stmt) -> Result<Vec<SqlRow>, StoreError> {
        self.pool.fetch(st).await.map_err(map_sql_err)
    }

    async fn execute(&self, st: Stmt) -> Result<u64, StoreError> {
        self.pool.execute(st).await.map_err(map_sql_err)
    }

    async fn begin(&self) -> Result<Tx, StoreError> {
        self.pool.begin().await.map_err(map_sql_err)
    }

    /// The `SELECT … FROM mem_fact f WHERE f.id = $id AND <read filter>` statement `get_fact` runs.
    fn get_fact_stmt(&self, ctx: &ScopeContext, id: &str) -> Stmt {
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        let p_id = st.text(id);
        let read = fact_read_filter(&mut st, ctx);
        st.sql(&format!(
            "SELECT {cols} FROM mem_fact f WHERE f.id = {p_id} AND ({read})"
        ));
        st
    }

    /// The tenant's facts, by collection, for one recall: from the search cache, after reading the
    /// facts it lacks (every fact on the tenant's first recall, else those written since).
    async fn search_snapshot(&self, tenant: &str) -> Result<cache::Snapshot, StoreError> {
        for _ in 0..SearchCache::load_attempts() {
            match self.embeddings.plan(tenant) {
                Plan::Fresh(snapshot) => return Ok(snapshot),
                Plan::Load { ids, ticket } => {
                    let rows = self.load_search_facts(tenant, ids.as_deref()).await?;
                    if let Some(snapshot) = self.embeddings.install(ticket, rows, ids.as_deref()) {
                        return Ok(snapshot);
                    }
                }
            }
        }
        // Overtaken on every attempt (erasures or newer loads): read the tenant once more for this
        // recall alone.
        Ok(SearchCache::uncached(
            self.load_search_facts(tenant, None).await?,
        ))
    }

    /// The search-cache form of `tenant`'s facts: every one when `ids` is `None` (read in id-ordered
    /// pages), else those of `ids` that still exist.
    async fn load_search_facts(
        &self,
        tenant: &str,
        ids: Option<&[String]>,
    ) -> Result<Vec<LoadedFact>, StoreError> {
        let cols = format!(
            "f.id AS id, f.owner_user AS owner_user, f.owner_team AS owner_team, \
             f.visibility AS visibility, f.memory_class AS memory_class, f.valid_from AS valid_from, \
             f.valid_to AS valid_to, f.ingested_at AS ingested_at, {}, f.collection AS collection, \
             f.embedding AS embedding",
            json_col(self.dialect, "f.entities", "entities")
        );
        let mut out = Vec::new();
        // Owner and entity names repeat across a tenant's facts; each distinct one is held once.
        let mut names: HashMap<String, Arc<str>> = HashMap::new();
        match ids {
            Some(ids) => {
                for chunk in ids.chunks(EMBEDDING_LOAD_CHUNK) {
                    let mut st = self.stmt();
                    let p_t = st.text(tenant);
                    let in_ids = st.in_list("f.id", chunk);
                    st.sql(&format!(
                        "SELECT {cols} FROM mem_fact f WHERE f.tenant = {p_t} AND {in_ids}"
                    ));
                    for row in self.fetch(st).await? {
                        out.push(self.search_fact(&row, &mut names)?);
                    }
                }
            }
            None => {
                // Keyset pages by id, compared and ordered under the same (default) collation, so
                // no page holds the whole tenant's embeddings twice over.
                let mut after: Option<String> = None;
                loop {
                    let mut st = self.stmt();
                    let p_t = st.text(tenant);
                    let page = match &after {
                        Some(last) => format!(" AND f.id > {}", st.text(last)),
                        None => String::new(),
                    };
                    let p_n = st.int(SEARCH_LOAD_PAGE);
                    st.sql(&format!(
                        "SELECT {cols} FROM mem_fact f WHERE f.tenant = {p_t}{page} \
                         ORDER BY f.id LIMIT {p_n}"
                    ));
                    let rows = self.fetch(st).await?;
                    let n = rows.len();
                    for row in rows {
                        out.push(self.search_fact(&row, &mut names)?);
                    }
                    if n < SEARCH_LOAD_PAGE as usize {
                        break;
                    }
                    after = out.last().map(|f| f.id.clone());
                }
            }
        }
        Ok(out)
    }

    /// One row of [`Self::load_search_facts`] in the cache's form.
    fn search_fact(
        &self,
        r: &SqlRow,
        names: &mut HashMap<String, Arc<str>>,
    ) -> Result<LoadedFact, StoreError> {
        let mut name = |s: String| -> Arc<str> {
            names
                .entry(s)
                .or_insert_with_key(|k| Arc::from(k.as_str()))
                .clone()
        };
        let embedding = match r.bytes("embedding")? {
            None => cache::Embedding::Absent,
            Some(raw) if raw.len() != self.embed_dim as usize * 4 => {
                cache::Embedding::WrongDim(raw.len() / 4)
            }
            Some(raw) => {
                let values = decode_embedding(&raw, self.embed_dim)?;
                cache::Embedding::Vector {
                    norm_sq: cache::norm_sq(&values),
                    values: Arc::from(values),
                }
            }
        };
        Ok(LoadedFact {
            id: r.text("id")?,
            collection: r.text("collection")?,
            fact: cache::CachedFact {
                owner_user: name(r.text("owner_user")?),
                owner_team: r.opt_text("owner_team")?.map(&mut name),
                visibility: parse_visibility(&r.text("visibility")?)?,
                memory_class: parse_memory_class(&r.text("memory_class")?)?,
                valid_from: r.ts("valid_from")?,
                valid_to: r.opt_ts("valid_to")?,
                ingested_at: r.ts("ingested_at")?,
                entities: string_list(r.json("entities")?, "entities")?
                    .into_iter()
                    .map(&mut name)
                    .collect(),
                embedding,
            },
        })
    }

    /// The stage-1 predicates of `ctx` and `q` in the search cache's form: those of
    /// [`search_read_filter`], [`validity_clause`] and [`meta_clause`]. The entity predicate's fact ids
    /// are read here, once per recall.
    async fn search_filter(
        &self,
        ctx: &ScopeContext,
        q: &StageOneQuery,
    ) -> Result<SearchFilter, StoreError> {
        let entity_facts = match &q.filters.entity {
            Some(e) => {
                let mut st = self.stmt();
                let p = st.text(e);
                st.sql(&format!(
                    "SELECT fe.fact_id AS id FROM mem_fact_entity fe WHERE fe.entity_id = {p}"
                ));
                Some(
                    self.fetch(st)
                        .await?
                        .iter()
                        .map(|r| r.text("id"))
                        .collect::<Result<HashSet<_>, _>>()?,
                )
            }
            None => None,
        };
        Ok(SearchFilter {
            memory_class: q.filters.memory_class,
            visibility: q.filters.visibility,
            entity_facts,
            ..self.entity_run_filter(ctx, q)
        })
    }

    /// The predicates of a graph or link run in the search cache's form: the read filter of the
    /// stage-1 signals and validity, without the metadata predicates.
    fn entity_run_filter(&self, ctx: &ScopeContext, q: &StageOneQuery) -> SearchFilter {
        SearchFilter {
            user: ctx.user.clone(),
            teams: ctx.teams.iter().cloned().collect(),
            valid_at: q
                .filters
                .valid_at
                .map(|at| at_stored_precision(self.dialect, at)),
            memory_class: None,
            visibility: None,
            entity_facts: None,
        }
    }

    /// The statement of one keyword run of step 9′.3: `(fact id, raw score)`, best first, at most `k`.
    fn keyword_stmt(
        &self,
        ctx: &ScopeContext,
        q: &StageOneQuery,
        terms: &[String],
        k: i64,
        collection: Option<&str>,
    ) -> Stmt {
        let mut st = self.stmt();
        let read = search_read_filter(&mut st, ctx);
        let validity = validity_clause(&mut st, q.filters.valid_at);
        let meta = meta_clause(&mut st, q);
        let coll = collection_clause(&mut st, collection);
        match self.dialect {
            SqlDialect::Sqlite => {
                // The terms travel as one bound parameter, each quoted as an FTS5 string so no term
                // is read as FTS5 query syntax (§Performance, Security: "quoted as FTS5 strings").
                let terms = st.text(fts5_terms(terms));
                let p_k = st.int(k);
                st.sql(&format!(
                    // `CROSS JOIN` fixes the full-text table as the outer loop. With the collection
                    // conjunct the planner otherwise drives from `mem_fact_collection` and evaluates
                    // `MATCH` once per row of the collection (measured: ~270 ms a run at 20,000 facts).
                    "SELECT f.id AS id, -bm25(mem_fact_fts) AS s FROM mem_fact_fts \
                     CROSS JOIN mem_fact f \
                     WHERE f.rowid = mem_fact_fts.rowid AND mem_fact_fts MATCH {terms} \
                     AND ({read}) AND {validity}{meta}{coll} \
                     ORDER BY s DESC LIMIT {p_k}"
                ));
            }
            SqlDialect::Postgres => {
                let terms = st.text(terms.join(" ").replace('-', " "));
                let p_k = st.int(k);
                st.sql(&format!(
                    "SELECT f.id AS id, \
                     ts_rank_cd(f.content_tsv, plainto_tsquery('simple', {terms}))::float8 AS s \
                     FROM mem_fact f \
                     WHERE f.content_tsv @@ plainto_tsquery('simple', {terms}) \
                     AND ({read}) AND {validity}{meta}{coll} ORDER BY s DESC LIMIT {p_k}"
                ));
            }
        }
        st
    }

    /// Write a fact's entity and derived-from junction rows to match `f`, inside `tx`.
    async fn write_fact_links(&self, tx: &mut Tx, f: &Fact) -> Result<(), StoreError> {
        for table in ["mem_fact_entity", "mem_fact_derived"] {
            let mut st = self.stmt();
            let p = st.text(&f.id);
            st.sql(&format!("DELETE FROM {table} WHERE fact_id = {p}"));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        for e in &f.entities {
            let mut st = self.stmt();
            let (p_f, p_e) = (st.text(&f.id), st.text(e));
            st.sql(&format!(
                "INSERT INTO mem_fact_entity (fact_id, entity_id) VALUES ({p_f}, {p_e}) \
                 ON CONFLICT DO NOTHING"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        for d in &f.derived_from {
            let mut st = self.stmt();
            let (p_f, p_d) = (st.text(&f.id), st.text(d));
            st.sql(&format!(
                "INSERT INTO mem_fact_derived (fact_id, derived_from) VALUES ({p_f}, {p_d}) \
                 ON CONFLICT DO NOTHING"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        Ok(())
    }

    /// Count the rows of one tenant-carrying table that belong to `tenant`.
    ///
    /// `table` comes from `ERASURE_SET` — a compile-time constant, never caller input — so naming it in
    /// the statement text is not a caller-controlled path; the tenant is bound (sql-safety layer).
    async fn count_tenant_rows(
        &self,
        tx: &mut Tx,
        table: &str,
        tenant: &str,
    ) -> Result<u64, StoreError> {
        let mut st = self.stmt();
        let p = st.text(tenant);
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM {table} WHERE tenant = {p}"
        ));
        let rows = tx.fetch(st).await.map_err(map_sql_err)?;
        count_of(&rows)
    }

    /// Count the rows of one table of `SCOPE_ERASURE_SET` that belong to `tenant` and whose team
    /// expression `team_col` is `team`. `table` and `team_col` are compile-time constants; the tenant
    /// and team are bound (sql-safety layer).
    async fn count_team_rows(
        &self,
        tx: &mut Tx,
        table: &str,
        team_col: &str,
        tenant: &str,
        team: &str,
    ) -> Result<u64, StoreError> {
        let mut st = self.stmt();
        let (p_t, p_team) = (st.text(tenant), st.text(team));
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM {table} WHERE tenant = {p_t} AND {team_col} = {p_team}"
        ));
        let rows = tx.fetch(st).await.map_err(map_sql_err)?;
        count_of(&rows)
    }
}

/// The collation an id is ordered and compared under: byte order in both dialects, so a keyset
/// page boundary is the same wherever the database's default collation is locale-aware (PostgreSQL).
fn id_collation(d: SqlDialect) -> &'static str {
    match d {
        SqlDialect::Sqlite => "",
        SqlDialect::Postgres => " COLLATE \"C\"",
    }
}

// --- Read filters and recall fragments --------------------------------------------------------------
//
// Each builder binds its own parameters into the statement and returns a fragment over the `f`
// (fact), or the given, table alias. Only constants and placeholders reach the SQL text.

/// Whether `ctx` is the C7 maintenance scope: the `"maintenance"` jti marker with an empty user. Such
/// a scope reaches the whole tenant — never another tenant (RISK-009, RISK-017).
fn is_maintenance_scope(ctx: &ScopeContext) -> bool {
    ctx.token_jti == "maintenance" && ctx.user.is_empty()
}

/// The fact read filter of every direct read (§2C.3): the caller's own facts, team-shared facts of
/// the caller's teams, tenant-shared facts, and facts an active consent grant names the caller on —
/// all within the caller's tenant. A maintenance scope reads the whole tenant.
fn fact_read_filter(st: &mut Stmt, ctx: &ScopeContext) -> String {
    let p_tenant = st.text(&ctx.tenant);
    if is_maintenance_scope(ctx) {
        return format!("f.tenant = {p_tenant}");
    }
    let p_user = st.text(&ctx.user);
    let teams = st.in_list("f.owner_team", &ctx.teams);
    let p_now = st.ts(Utc::now());
    format!(
        "f.tenant = {p_tenant} AND (f.owner_user = {p_user} \
         OR (f.visibility = 'team-shared' AND {teams}) \
         OR f.visibility = 'tenant-shared' \
         OR f.id IN (SELECT g.fact FROM mem_grant g WHERE g.tenant = {p_tenant} \
                     AND g.grantee = {p_user} AND g.revoked_at IS NULL \
                     AND g.valid_from <= {p_now} AND g.valid_to > {p_now}))"
    )
}

/// The read filter of the three stage-1 signals: the three scope arms without the consent-grant arm
/// (SA-GRANT-08). A fact the caller reaches only through a grant is fetched by id and found by no
/// search leg — as in the SurrealDB store, whose collective-profile signals share this filter.
// `SqlStorage::new` takes no `MemoryProfile`, so the storage applies the collective
// profile's filter (the in-process value of `memory_profile`, MemoryService spec §5); the individual
// profile's narrower owner-only filter is not offered.
fn search_read_filter(st: &mut Stmt, ctx: &ScopeContext) -> String {
    let p_tenant = st.text(&ctx.tenant);
    let p_user = st.text(&ctx.user);
    let teams = st.in_list("f.owner_team", &ctx.teams);
    format!(
        "f.tenant = {p_tenant} AND (f.owner_user = {p_user} \
         OR (f.visibility = 'team-shared' AND {teams}) \
         OR f.visibility = 'tenant-shared')"
    )
}

/// The reduced read filter of the tables with no visibility: a source's reads, and the first step of
/// a neighbour hop. Entities and relationships are read through [`entity_read_filter`] and
/// [`rel_read_filter`].
fn novis_filter(st: &mut Stmt, ctx: &ScopeContext, alias: &str) -> String {
    let p_tenant = st.text(&ctx.tenant);
    let p_user = st.text(&ctx.user);
    let teams = st.in_list(&format!("{alias}.owner_team"), &ctx.teams);
    format!("{alias}.tenant = {p_tenant} AND ({alias}.owner_user = {p_user} OR {teams})")
}

/// The read filter of an entity (alias `alias`), its names and its aliases: the caller's own, or named
/// by a fact the caller can read. A link grants nothing and cannot reveal what the fact filter hides
/// (SA-GRAPH-05); team membership alone reads no entity. A maintenance scope reads the whole tenant.
// spec: components/memory-store#SA-GRAPH-05
// Read by team, names and links from someone's private fact would reach every teammate. These filters
// apply on every read of a record; a neighbour hop keeps `novis_filter` and adds SA-GRAPH-05's
// asserting-fact check.
fn entity_read_filter(st: &mut Stmt, ctx: &ScopeContext, alias: &str) -> String {
    let p_tenant = st.text(&ctx.tenant);
    if is_maintenance_scope(ctx) {
        return format!("{alias}.tenant = {p_tenant}");
    }
    let p_user = st.text(&ctx.user);
    let read = fact_read_filter(st, ctx);
    format!(
        "{alias}.tenant = {p_tenant} AND ({alias}.owner_user = {p_user} OR EXISTS \
         (SELECT 1 FROM mem_fact_entity fe JOIN mem_fact f ON f.id = fe.fact_id \
          WHERE fe.entity_id = {alias}.id AND ({read})))"
    )
}

/// The read filter of a relationship (alias `alias`): the caller's own, recorded by a fact the caller
/// can read (either role), or a person's link of the caller's team not held to its owner.
fn rel_read_filter(st: &mut Stmt, ctx: &ScopeContext, alias: &str) -> String {
    let p_tenant = st.text(&ctx.tenant);
    if is_maintenance_scope(ctx) {
        return format!("{alias}.tenant = {p_tenant}");
    }
    let p_user = st.text(&ctx.user);
    let teams = st.in_list(&format!("{alias}.owner_team"), &ctx.teams);
    let read = fact_read_filter(st, ctx);
    format!(
        "{alias}.tenant = {p_tenant} AND ({alias}.owner_user = {p_user} \
         OR ({alias}.replaces IS NOT NULL AND NOT {alias}.owner_only AND {teams}) \
         OR EXISTS (SELECT 1 FROM mem_relationship_fact lf JOIN mem_fact f ON f.id = lf.fact_id \
          WHERE lf.relationship_id = {alias}.id AND ({read})))"
    )
}

/// `row` as `ctx` sees it (`Entity::as_seen_by`).
fn entity_for(ctx: &ScopeContext, row: &SqlRow) -> Result<Entity, StoreError> {
    let e = row_to_entity(row)?;
    let own = owner_view(ctx, &e.owner.user);
    Ok(e.as_seen_by(own))
}

/// The table holding `id`, by its `table:` prefix, among those whose rows have an owner.
fn owned_table(id: &str) -> Option<&'static str> {
    match id.split_once(':')?.0 {
        "fact" => Some("mem_fact"),
        "entity" => Some("mem_entity"),
        "relationship" => Some("mem_relationship"),
        "source" => Some("mem_source"),
        _ => None,
    }
}

/// The bi-temporal validity predicate: as of `valid_at` when set, else currently valid.
fn validity_clause(st: &mut Stmt, valid_at: Option<DateTime<Utc>>) -> String {
    match valid_at {
        Some(at) => {
            let p = st.ts(at);
            format!("f.valid_from <= {p} AND (f.valid_to IS NULL OR f.valid_to > {p})")
        }
        None => "f.valid_to IS NULL".to_string(),
    }
}

/// The optional metadata predicates (memory_class / visibility / entity) of the vector and keyword
/// signals.
fn meta_clause(st: &mut Stmt, q: &StageOneQuery) -> String {
    let mut meta = String::new();
    if let Some(mc) = q.filters.memory_class {
        let p = st.text(memory_class_str(mc));
        meta.push_str(&format!(" AND f.memory_class = {p}"));
    }
    if let Some(v) = q.filters.visibility {
        let p = st.text(visibility_str(v));
        meta.push_str(&format!(" AND f.visibility = {p}"));
    }
    if let Some(e) = &q.filters.entity {
        let p = st.text(e);
        meta.push_str(&format!(
            " AND f.id IN (SELECT fe.fact_id FROM mem_fact_entity fe WHERE fe.entity_id = {p})"
        ));
    }
    meta
}

/// The collection conjunct of a per-collection run (step 9′.3, SA-COLL-05); empty for a run over every
/// collection.
fn collection_clause(st: &mut Stmt, collection: Option<&str>) -> String {
    match collection {
        Some(c) => {
            let p = st.text(c);
            format!(" AND f.collection = {p}")
        }
        None => String::new(),
    }
}

/// How many facts one page of a tenant's search-cache load reads (about 7.5 MB of embeddings at 384
/// dimensions).
const SEARCH_LOAD_PAGE: i64 = 5_000;

/// A vector run over a collection of at least this many cached facts runs on a blocking thread,
/// concurrently with the recall's other runs; a smaller one costs less than the thread hand-off.
const PARALLEL_SCAN_MIN: usize = 2_048;

/// What one run of step 9′.3 returns.
type RunResult = Result<Vec<(String, f64)>, StoreError>;

/// `at` as the dialect stores and compares it: RFC 3339 milliseconds in SQLite (`exec::ts_text`),
/// microseconds since 2000-01-01 in PostgreSQL (as `sqlx` encodes a bound `TIMESTAMPTZ`). The search
/// cache compares a fact's stored validity with `filters.valid_at` at this precision, as the SQL
/// predicate did.
fn at_stored_precision(d: SqlDialect, at: DateTime<Utc>) -> DateTime<Utc> {
    match d {
        SqlDialect::Sqlite => DateTime::parse_from_rfc3339(&exec::ts_text(at))
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or(at),
        SqlDialect::Postgres => {
            let epoch = DateTime::<Utc>::from_naive_utc_and_offset(
                chrono::NaiveDate::from_ymd_opt(2000, 1, 1)
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .unwrap_or_default(),
                Utc,
            );
            match (at - epoch).num_microseconds() {
                Some(us) => epoch + chrono::Duration::microseconds(us),
                None => at,
            }
        }
    }
}

/// How many fact ids one search-cache reload binds — well inside SQLite's and PostgreSQL's parameter
/// limits.
const EMBEDDING_LOAD_CHUNK: usize = 500;

/// The FTS5 query for `terms`: each term a quoted FTS5 string (a `"` doubled inside it), the strings
/// separated by spaces — FTS5's implicit AND, as `plainto_tsquery` is in PostgreSQL.
fn fts5_terms(terms: &[String]) -> String {
    terms
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

// --- Row mapping -------------------------------------------------------------------------------------

/// The `mem_fact` select list (alias `f`), with the persistence-only columns left out and the
/// embedding's presence projected as `has_embedding`.
fn fact_cols(d: SqlDialect) -> String {
    format!(
        "f.id, f.tenant, f.owner_team, f.owner_user, {}, {}, f.source_id, f.memory_class, \
         f.fact_class, f.visibility, f.valid_from, f.valid_to, f.ingested_at, f.confidence, \
         f.salience, f.stability, f.pii_review, f.supersedes, f.superseded_by, {}, \
         f.last_recalled_at, f.collection, f.embedding IS NOT NULL AS has_embedding",
        json_col(d, "f.content", "content"),
        json_col(d, "f.entities", "entities"),
        json_col(d, "f.derived_from", "derived_from"),
    )
}

fn entity_cols(d: SqlDialect) -> String {
    format!(
        "e.id, e.tenant, e.owner_team, e.owner_user, e.canonical_name, {}, e.collection, {}, {}",
        json_col(d, "e.aliases", "aliases"),
        json_col(d, "e.private_aliases", "private_aliases"),
        json_col(d, "e.alias_folders", "alias_folders")
    )
}

const RELATIONSHIP_COLS: &str = "r.id, r.tenant, r.owner_team, r.owner_user, r.kind, r.from_ent, \
     r.to_ent, r.valid_from, r.valid_to, r.ingested_at, r.confidence, r.source_id, r.collection, \
     r.replaces, r.owner_only, r.author";

const SOURCE_COLS: &str =
    "s.id, s.tenant, s.owner_team, s.owner_user, s.origin_ref, s.modification_marker, s.trust_signal, \
     s.collection";

fn owner_of(r: &SqlRow) -> Result<ScopeRef, StoreError> {
    Ok(ScopeRef {
        tenant: r.text("tenant")?,
        team: r.opt_text("owner_team")?,
        user: r.text("owner_user")?,
    })
}

fn string_list(v: Json, col: &str) -> Result<Vec<String>, StoreError> {
    serde_json::from_value(v).map_err(|e| StoreError::Internal(format!("decode {col}: {e}")))
}

fn row_to_fact(r: &SqlRow) -> Result<Fact, StoreError> {
    Ok(Fact {
        id: r.text("id")?,
        content: r.json("content")?,
        entities: string_list(r.json("entities")?, "entities")?,
        source_id: r.opt_text("source_id")?,
        memory_class: parse_memory_class(&r.text("memory_class")?)?,
        fact_class: r.opt_text("fact_class")?,
        visibility: parse_visibility(&r.text("visibility")?)?,
        owner: owner_of(r)?,
        valid_from: r.ts("valid_from")?,
        valid_to: r.opt_ts("valid_to")?,
        ingested_at: r.ts("ingested_at")?,
        confidence: r.float("confidence")?,
        salience: r.float("salience")?,
        stability: r.float("stability")?,
        pii_review: r.boolean("pii_review")?,
        supersedes: r.opt_text("supersedes")?,
        superseded_by: r.opt_text("superseded_by")?,
        derived_from: string_list(r.json("derived_from")?, "derived_from")?,
        last_recalled_at: r.opt_ts("last_recalled_at")?,
        collection: r.text("collection")?,
    })
}

/// `alias_folders` in its stored form.
fn folders_json(e: &Entity) -> Result<Json, StoreError> {
    serde_json::to_value(&e.alias_folders)
        .map_err(|err| StoreError::Internal(format!("encode alias_folders: {err}")))
}

fn row_to_entity(r: &SqlRow) -> Result<Entity, StoreError> {
    let mut aliases = string_list(r.json("aliases")?, "aliases")?;
    let private_aliases = string_list(r.json("private_aliases")?, "private_aliases")?;
    aliases.extend(private_aliases.iter().cloned());
    Ok(Entity {
        id: r.text("id")?,
        canonical_name: r.text("canonical_name")?,
        aliases,
        owner: owner_of(r)?,
        collection: r.text("collection")?,
        private_aliases,
        alias_folders: serde_json::from_value(r.json("alias_folders")?)
            .map_err(|e| StoreError::Internal(format!("decode alias_folders: {e}")))?,
    })
}

fn row_to_relationship(r: &SqlRow) -> Result<Relationship, StoreError> {
    Ok(Relationship {
        id: r.text("id")?,
        kind: r.text("kind")?,
        from: r.text("from_ent")?,
        to: r.text("to_ent")?,
        valid_from: r.ts("valid_from")?,
        valid_to: r.opt_ts("valid_to")?,
        ingested_at: r.ts("ingested_at")?,
        confidence: r.float("confidence")?,
        source_id: r.opt_text("source_id")?,
        owner: owner_of(r)?,
        collection: r.text("collection")?,
        replaces: r.opt_text("replaces")?,
        owner_only: r.boolean("owner_only")?,
        author: r.opt_text("author")?,
    })
}

fn row_to_source(r: &SqlRow) -> Result<Source, StoreError> {
    Ok(Source {
        id: r.text("id")?,
        origin_ref: r.text("origin_ref")?,
        modification_marker: r.opt_text("modification_marker")?,
        trust_signal: r.float("trust_signal")?,
        owner: owner_of(r)?,
        collection: r.text("collection")?,
    })
}

/// The `n` column of a `SELECT COUNT(*) AS n` result.
fn count_of(rows: &[SqlRow]) -> Result<u64, StoreError> {
    match rows.first() {
        Some(r) => Ok(u64::try_from(r.int("n")?).unwrap_or(0)),
        None => Ok(0),
    }
}

/// The canonical kebab-case string for a `MemoryClass` (the wire form and the §B `CHECK` values).
fn memory_class_str(c: MemoryClass) -> &'static str {
    match c {
        MemoryClass::Episodic => "episodic",
        MemoryClass::Semantic => "semantic",
        MemoryClass::Consolidated => "consolidated",
    }
}

fn parse_memory_class(s: &str) -> Result<MemoryClass, StoreError> {
    match s {
        "episodic" => Ok(MemoryClass::Episodic),
        "semantic" => Ok(MemoryClass::Semantic),
        "consolidated" => Ok(MemoryClass::Consolidated),
        other => Err(StoreError::Internal(format!(
            "decode memory_class: {other}"
        ))),
    }
}

/// The canonical kebab-case string for a `Visibility`.
fn visibility_str(v: Visibility) -> &'static str {
    match v {
        Visibility::UserPrivate => "user-private",
        Visibility::TeamShared => "team-shared",
        Visibility::TenantShared => "tenant-shared",
    }
}

fn parse_visibility(s: &str) -> Result<Visibility, StoreError> {
    match s {
        "user-private" => Ok(Visibility::UserPrivate),
        "team-shared" => Ok(Visibility::TeamShared),
        "tenant-shared" => Ok(Visibility::TenantShared),
        other => Err(StoreError::Internal(format!("decode visibility: {other}"))),
    }
}

// --- Embeddings ----------------------------------------------------------------------------------------

/// An embedding as §B stores it: little-endian `f32`s.
fn encode_embedding(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Decode a stored embedding, refusing one whose length is not exactly `dim` values.
// `StoreError` has no variant for the MemoryService's `MEM_DIM_MISMATCH`. It is reported as
// `StoreError::Internal` with a message beginning "stored embedding dimension", which the Memory
// Service can recognise; `ready` checks for such a row as well.
fn decode_embedding(raw: &[u8], dim: u32) -> Result<Vec<f32>, StoreError> {
    if raw.len() != dim as usize * 4 {
        return Err(StoreError::Internal(format!(
            "stored embedding dimension {} != MEMORY_EMBED_DIM {dim}",
            raw.len() / 4
        )));
    }
    Ok(raw
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

/// Cosine similarity in [-1, 1]; 0 when either vector has no length — the exact scan's formula before
/// the search cache, kept as the reference `cache::vector_run` is checked against.
#[cfg(test)]
fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (f64::from(*x), f64::from(*y));
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

// --- Validation (the SurrealDB store's rules, unchanged) ---------------------------------------------

/// Validate a fact against the binding domain rules. A score-range failure says "range" so the X1
/// mapping selects `VAL_OUT_OF_RANGE` (else `VAL_INVALID_BODY`).
fn validate_fact(f: &Fact) -> Result<(), StoreError> {
    if f.entities.is_empty() {
        return Err(StoreError::Validation("entities must be non-empty".into()));
    }
    if !f.content.is_object() {
        return Err(StoreError::Validation(
            "content must be a JSON object".into(),
        ));
    }
    for (name, v) in [("confidence", f.confidence), ("salience", f.salience)] {
        if !(0.0..=1.0).contains(&v) {
            return Err(StoreError::Validation(format!("{name} out of range")));
        }
    }
    if f.stability < 0.0 {
        return Err(StoreError::Validation("stability out of range".into()));
    }
    if let Some(to) = f.valid_to {
        if to < f.valid_from {
            return Err(StoreError::Validation("valid_to before valid_from".into()));
        }
    }
    validate_collection(&f.collection)
}

/// Refuse a `collection` breaking SA-COLL-01 on a write, as `VAL_INVALID_BODY` (SA-COLL-05).
// spec: components/memory-store#SA-COLL-01
fn validate_collection(label: &str) -> Result<(), StoreError> {
    if crate::types::domain::is_valid_collection(label) {
        Ok(())
    } else {
        Err(StoreError::Validation("collection".into()))
    }
}

// --- Erasure -------------------------------------------------------------------------------------------

/// Every table of §B that carries a tenant, each in its `tenant` column — the erasure predicate set
/// (ADR-018). It must stay exhaustive: a table added to §B without an entry here leaves the tenant's
/// rows behind after `erase_tenant` reports success. The two junction tables carry no tenant; their
/// rows go with their fact.
const ERASURE_SET: &[&str] = &[
    "mem_audit_log",
    "mem_dead_letter",
    // Before `mem_entity`, whose foreign key would otherwise cascade the rows away uncounted.
    "mem_entity_name",
    "mem_entity",
    "mem_fact",
    "mem_grant",
    "mem_idempotency_record",
    "mem_job_fact",
    "mem_maintenance_state",
    "mem_quarantine",
    "mem_relationship",
    "mem_source",
    "mem_work_job",
];

/// Every table of §B that carries an owner team, with the expression naming the team in SQLite and in
/// PostgreSQL — the predicate set of `erase_scope` (ordin change M-6 c). The audit and dead-letter
/// tables hold the team inside their `scope` JSON object. `mem_idempotency_record` and
/// `mem_maintenance_state` carry no team and are not in it; every other table of `ERASURE_SET` is. The
/// junction tables go with their fact.
const SCOPE_ERASURE_SET: &[(&str, &str, &str)] = &[
    (
        "mem_audit_log",
        "json_extract(scope, '$.team')",
        "scope->>'team'",
    ),
    (
        "mem_dead_letter",
        "json_extract(scope, '$.team')",
        "scope->>'team'",
    ),
    ("mem_entity_name", "owner_team", "owner_team"),
    ("mem_entity", "owner_team", "owner_team"),
    ("mem_fact", "owner_team", "owner_team"),
    ("mem_grant", "grantor_team", "grantor_team"),
    ("mem_job_fact", "scope_team", "scope_team"),
    ("mem_quarantine", "owner_team", "owner_team"),
    ("mem_relationship", "owner_team", "owner_team"),
    ("mem_source", "owner_team", "owner_team"),
    ("mem_work_job", "scope_team", "scope_team"),
];

/// The lines a `TenantErasureProof` digest is taken over (SA-ERASE-01): the tenant, the namespace,
/// then one `<table>=<count>` line per table in key order.
fn erasure_digest_input(
    tenant: &str,
    namespace: &str,
    removed: &BTreeMap<String, u64>,
) -> Vec<String> {
    let mut lines = Vec::with_capacity(removed.len() + 2);
    lines.push(format!("tenant={tenant}"));
    lines.push(format!("namespace={namespace}"));
    for (table, count) in removed {
        lines.push(format!("{table}={count}"));
    }
    lines
}

/// SHA-256 hex over the newline-joined, already-sorted lines (SA-DELETE-01 digest).
fn sha256_hex(sorted: &[String]) -> String {
    let digest = Sha256::digest(sorted.join("\n").as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[async_trait]
impl MemoryStore for SqlStorage {
    async fn put_fact(&self, f: &Fact) -> Result<(), StoreError> {
        validate_fact(f)?;
        let mut st = self.stmt();
        let vals = [
            st.text(&f.id),
            st.text(&f.owner.tenant),
            st.opt_text(f.owner.team.clone()),
            st.text(&f.owner.user),
            st.json(&f.content),
            st.text(crate::store::content_to_text(&f.content)),
            st.json(&Json::from(f.entities.clone())),
            st.opt_text(f.source_id.clone()),
            st.text(memory_class_str(f.memory_class)),
            st.opt_text(f.fact_class.clone()),
            st.text(visibility_str(f.visibility)),
            st.ts(f.valid_from),
            st.opt_ts(f.valid_to),
            st.ts(f.ingested_at),
            st.float(f.confidence),
            st.float(f.salience),
            st.float(f.stability),
            st.boolean(f.pii_review),
            st.opt_text(f.supersedes.clone()),
            st.opt_text(f.superseded_by.clone()),
            st.json(&Json::from(f.derived_from.clone())),
            st.opt_ts(f.last_recalled_at),
            st.text(&f.collection),
        ]
        .join(", ");
        // An upsert replaces the whole row, as the SurrealDB store's `UPSERT … CONTENT` does: the
        // embedding is cleared and `set_fact_embedding` fills it. The owner is never updated, and the
        // `WHERE` refuses an id that already belongs to another tenant or user.
        st.sql(&format!(
            "INSERT INTO mem_fact (id, tenant, owner_team, owner_user, content, content_text, \
             entities, source_id, memory_class, fact_class, visibility, valid_from, valid_to, \
             ingested_at, confidence, salience, stability, pii_review, supersedes, superseded_by, \
             derived_from, last_recalled_at, collection, embedding, embedding_model) \
             VALUES ({vals}, NULL, NULL) \
             ON CONFLICT (id) DO UPDATE SET content = excluded.content, \
             content_text = excluded.content_text, entities = excluded.entities, \
             source_id = excluded.source_id, memory_class = excluded.memory_class, \
             fact_class = excluded.fact_class, visibility = excluded.visibility, \
             valid_from = excluded.valid_from, valid_to = excluded.valid_to, \
             ingested_at = excluded.ingested_at, confidence = excluded.confidence, \
             salience = excluded.salience, stability = excluded.stability, \
             pii_review = excluded.pii_review, supersedes = excluded.supersedes, \
             superseded_by = excluded.superseded_by, derived_from = excluded.derived_from, \
             last_recalled_at = excluded.last_recalled_at, collection = excluded.collection, \
             embedding = NULL, \
             embedding_model = NULL \
             WHERE mem_fact.tenant = excluded.tenant \
             AND mem_fact.owner_user = excluded.owner_user"
        ));
        let mut tx = self.begin().await?;
        if tx.execute(st).await.map_err(map_sql_err)? == 0 {
            return Err(StoreError::ScopeForbidden);
        }
        self.write_fact_links(&mut tx, f).await?;
        tx.commit().await.map_err(map_sql_err)?;
        // The upsert cleared the embedding.
        self.embeddings
            .forget(&f.owner.tenant, std::slice::from_ref(&f.id));
        tracing::debug!(target: "memory", table = "mem_fact", "store.put");
        Ok(())
    }

    async fn get_fact(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Fact>, StoreError> {
        let rows = self.fetch(self.get_fact_stmt(ctx, id)).await?;
        rows.first().map(row_to_fact).transpose()
    }

    async fn recall(
        &self,
        ctx: &ScopeContext,
        q: &StageOneQuery,
    ) -> Result<Vec<Candidate>, StoreError> {
        let k = i64::from(q.stage1_k.max(1));

        // Step 9′.1 — the collections in play (SA-COLL-05: the SurrealDB store's rule, on
        // `mem_fact.collection`).
        // spec: components/memory-store#SA-COLL-05
        // The vector runs and the collections step read the search cache; a recall with neither —
        // only keyword lists over listed collections — never loads it.
        // The cache orders labels by byte, as SQLite's `ORDER BY` did and as fusion's tie-break on labels
        // does; PostgreSQL's `ORDER BY` followed the database's collation, which can place `:`, `_`
        // and `-` differently. The order reaches a result only through the graph cap (step 9″.5),
        // which trims runs of one hop in collection order.
        let mut lists = fusion::lists(q);
        let has_vector = lists.iter().any(|l| matches!(l.kind, ListKind::Vector(_)));
        let search = if has_vector || q.filters.collections.is_none() {
            let snapshot = self.search_snapshot(&ctx.tenant).await?;
            let filter = self.search_filter(ctx, q).await?;
            Some((snapshot, Arc::new(filter)))
        } else {
            None
        };
        let collections = match (&q.filters.collections, &search) {
            (Some(listed), _) => listed.clone(),
            (None, Some((snapshot, filter))) => cache::collections_in_play(snapshot, filter),
            // `search` is loaded whenever no collection is listed.
            (None, None) => Vec::new(),
        };
        tracing::debug!(target: "memory", count = collections.len(), "store.recall_collections");
        crate::store::graph::refuse_too_many_collections(
            q.filters.collections.is_some(),
            collections.len(),
        )?;

        // Steps 9′.2 and 9′.3 — one run per (list, collection), each filtered by its collection
        // before it is scored. The vector runs are exact scans whatever the number of collections,
        // which is SA-COLL-03's filter-first strategy. A failed run fails the whole recall.
        // Every embedding the read filter admits is scored: no pre-selection keeps the scan exact.
        // The runs are concurrent — each vector run of a large collection on a blocking thread over
        // the cached snapshot, each keyword run on its own pool connection — and are fused in (list, collection)
        // order, so the result, and the error a failed recall reports (the first in that order), are
        // those of running them in sequence.
        let mut runs = Vec::with_capacity(lists.len() * collections.len());
        let mut pending: tokio::task::JoinSet<(usize, RunResult)> = tokio::task::JoinSet::new();
        let mut done: Vec<Option<RunResult>> = Vec::new();
        for (list, l) in lists.iter().enumerate() {
            let query = match l.kind {
                ListKind::Vector(v) => Some((Arc::<[f32]>::from(v), cache::norm_sq(v))),
                _ => None,
            };
            for c in &collections {
                let slot = done.len();
                done.push(None);
                runs.push(fusion::Run {
                    list,
                    collection: c.clone(),
                    hits: Vec::new(),
                });
                match l.kind {
                    ListKind::Vector(_) => {
                        let Some((query, query_norm_sq)) = query.clone() else {
                            continue;
                        };
                        let (dim, k) = (self.embed_dim, usize::from(q.stage1_k.max(1)));
                        let slab = search.as_ref().and_then(|(s, _)| s.get(c).cloned());
                        let filter = search.as_ref().map(|(_, f)| f.clone());
                        let scan = move || -> RunResult {
                            if query.len() as u32 != dim {
                                return Err(StoreError::Validation(format!(
                                    "query vector dimension {} != MEMORY_EMBED_DIM {dim}",
                                    query.len()
                                )));
                            }
                            match (slab, filter) {
                                (Some(slab), Some(filter)) => {
                                    cache::vector_run(&slab, &filter, &query, query_norm_sq, dim, k)
                                }
                                _ => Ok(Vec::new()),
                            }
                        };
                        let large = search
                            .as_ref()
                            .and_then(|(s, _)| s.get(c))
                            .is_some_and(|slab| slab.len() >= PARALLEL_SCAN_MIN);
                        if large {
                            pending.spawn_blocking(move || (slot, scan()));
                        } else {
                            done[slot] = Some(scan());
                        }
                    }
                    ListKind::Keyword(terms) => {
                        let st = self.keyword_stmt(ctx, q, terms, k, Some(c));
                        let pool = self.pool.clone();
                        pending.spawn(async move {
                            let hits = match pool.fetch(st).await {
                                Ok(rows) => rows
                                    .iter()
                                    .map(|r| Ok((r.text("id")?, r.float("s")?)))
                                    .collect(),
                                Err(e) => Err(map_sql_err(e)),
                            };
                            (slot, hits)
                        });
                    }
                    ListKind::Graph | ListKind::Link(_) => done[slot] = Some(Ok(Vec::new())),
                }
            }
        }
        while let Some(joined) = pending.join_next().await {
            let (slot, hits) =
                joined.map_err(|e| StoreError::Internal(format!("recall run: {e}")))?;
            done[slot] = Some(hits);
        }
        for (run, hits) in runs.iter_mut().zip(done) {
            run.hits = hits.unwrap_or_else(|| Ok(Vec::new()))?;
        }

        // Step 9″ — the graph list and the link lists (SA-COLL-05: the SurrealDB store's rules).
        // spec: components/memory-store#SA-GRAPH-05
        let reach = self.graph_reach(ctx, q).await?;
        let mut reached = crate::store::graph::Reached::default();
        if let Some(reach) = &reach {
            // A keyword-only recall over listed collections has not read the search cache yet.
            let snapshot = match &search {
                Some((snapshot, _)) => snapshot.clone(),
                None => self.search_snapshot(&ctx.tenant).await?,
            };
            let filter = self.entity_run_filter(ctx, q);
            let mut plan: Vec<(ListKind<'static>, f64, u8, Vec<String>)> = Vec::new();
            if !reach.seeds.is_empty() {
                plan.push((
                    ListKind::Graph,
                    fusion::GRAPH_WEIGHT,
                    0,
                    reach.seeds.clone(),
                ));
            }
            for hop in 1..=q.graph_hops.min(2) {
                let ents = reach.at_hop(hop);
                if !ents.is_empty() {
                    plan.push((ListKind::Link(hop), fusion::link_weight(hop), hop, ents));
                }
            }
            for (kind, weight, hop, ents) in plan {
                let list = lists.len();
                lists.push(fusion::List { kind, weight });
                for c in &collections {
                    let wanted: HashSet<&str> = ents.iter().map(String::as_str).collect();
                    let hits = snapshot.get(c).map_or_else(Vec::new, |slab| {
                        cache::entity_run(slab, &filter, &wanted, usize::from(q.stage1_k.max(1)))
                    });
                    for (id, fact_entities) in &hits {
                        let via = if hop == 0 {
                            Vec::new()
                        } else {
                            reach.via_of(fact_entities)
                        };
                        reached.note(id, hop, via);
                    }
                    runs.push(fusion::Run {
                        list,
                        collection: c.clone(),
                        hits: fusion::ordered_hits(hits.into_iter().map(|(id, _)| id).collect()),
                    });
                }
            }
            fusion::cap_graph_runs(&lists, &mut runs, q.graph_cap);
        }
        let run_count = runs.len();

        // Step 10′ — fuse, quota and cap.
        let chosen = fusion::fuse(&lists, runs, q.stage1_k);
        if chosen.is_empty() {
            tracing::debug!(target: "memory", candidate_count = 0, collections = collections.len(), runs = run_count, "store.recall");
            return Ok(Vec::new());
        }

        // Step 10′.5 — resolve the chosen ids to their facts, re-applying the full read filter, in
        // one statement, and return them in fused order.
        let ids: Vec<String> = chosen.iter().map(|f| f.id.clone()).collect();
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        // `CROSS JOIN` fixes the id list as the outer loop on SQLite. Left to the planner this statement
        // walked the tenant's rows (measured: 40 ms against 0.5 ms for 50 ids of 60,000 facts).
        let (from, in_ids) = match self.dialect {
            SqlDialect::Sqlite => {
                let list = st.json_each(&ids);
                (
                    format!("json_each({list}) j CROSS JOIN mem_fact f"),
                    "f.id = j.value".to_string(),
                )
            }
            SqlDialect::Postgres => ("mem_fact f".to_string(), st.in_list("f.id", &ids)),
        };
        let read = fact_read_filter(&mut st, ctx);
        let validity = validity_clause(&mut st, q.filters.valid_at);
        st.sql(&format!(
            "SELECT {cols} FROM {from} WHERE {in_ids} AND ({read}) AND {validity}"
        ));
        let mut facts: HashMap<String, Fact> = HashMap::new();
        for row in self.fetch(st).await? {
            let fact = row_to_fact(&row)?;
            facts.insert(fact.id.clone(), fact);
        }
        // Step 9″.7 — the cited sources' trust, one batched read.
        let sources: Vec<String> = facts
            .values()
            .filter_map(|f| f.source_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let trusts = self.source_trusts(ctx, &sources).await?;
        let mut out = Vec::with_capacity(chosen.len());
        for f in chosen {
            if let Some(fact) = facts.remove(&f.id) {
                out.push(Candidate {
                    fact_id: fact.id.clone(),
                    source_trust: fact
                        .source_id
                        .as_ref()
                        .and_then(|s| trusts.get(s))
                        .copied()
                        .unwrap_or(1.0),
                    graph_hop: reached.hop.get(&fact.id).copied(),
                    via: reached.via.get(&fact.id).cloned().unwrap_or_default(),
                    fact,
                    semantic_score: f.semantic_score,
                    keyword_score: f.keyword_score,
                    graph_score: f.graph_score,
                    fused_score: f.fused_score,
                    fused_norm: f.fused_norm,
                });
            }
        }
        tracing::debug!(
            target: "memory",
            candidate_count = out.len(),
            collections = collections.len(),
            runs = run_count,
            graph_seeds = reach.as_ref().map_or(0, |r| r.seeds.len()),
            graph_reached = reach.as_ref().map_or(0, |r| r.reached.len()),
            "store.recall"
        );
        Ok(out)
    }

    async fn end_validity(
        &self,
        ctx: &ScopeContext,
        id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let Some(fact) = self.get_fact(ctx, id).await? else {
            return Err(StoreError::NotFound);
        };
        owner_gate(ctx, &fact.owner.user)?;
        let mut st = self.stmt();
        let (p_at, p_id, p_t) = (st.ts(at), st.text(id), st.text(&ctx.tenant));
        st.sql(&format!(
            "UPDATE mem_fact SET valid_to = {p_at} \
             WHERE id = {p_id} AND tenant = {p_t} AND valid_to IS NULL"
        ));
        // Step 11′, in the same transaction.
        let mut tx = self.begin().await?;
        tx.execute(st).await.map_err(map_sql_err)?;
        self.close_links(&mut tx, &ctx.tenant, id, at).await?;
        tx.commit().await.map_err(map_sql_err)?;
        // `valid_to` is a filter column of the search cache.
        self.embeddings.forget(&ctx.tenant, &[id.to_string()]);
        Ok(())
    }

    async fn supersede(
        &self,
        ctx: &ScopeContext,
        old_id: &str,
        new_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let (Some(old), Some(new)) = (
            self.get_fact(ctx, old_id).await?,
            self.get_fact(ctx, new_id).await?,
        ) else {
            return Err(StoreError::NotFound);
        };
        owner_gate(ctx, &old.owner.user)?;
        owner_gate(ctx, &new.owner.user)?;
        let mut tx = self.begin().await?;
        let mut st = self.stmt();
        let (p_at, p_new, p_old, p_t) = (
            st.ts(at),
            st.text(new_id),
            st.text(old_id),
            st.text(&ctx.tenant),
        );
        st.sql(&format!(
            "UPDATE mem_fact SET valid_to = {p_at}, superseded_by = {p_new} \
             WHERE id = {p_old} AND tenant = {p_t}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        let mut st = self.stmt();
        let (p_old, p_new, p_t) = (st.text(old_id), st.text(new_id), st.text(&ctx.tenant));
        st.sql(&format!(
            "UPDATE mem_fact SET supersedes = {p_old} WHERE id = {p_new} AND tenant = {p_t}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        // Step 12′: the new fact's links are already recorded, so a link it repeats stays live.
        self.close_links(&mut tx, &ctx.tenant, old_id, at).await?;
        tx.commit().await.map_err(map_sql_err)?;
        // The old fact's `valid_to` is a filter column of the search cache.
        self.embeddings.forget(&ctx.tenant, &[old_id.to_string()]);
        Ok(())
    }

    async fn hard_delete(&self, ctx: &ScopeContext, id: &str) -> Result<DeletionProof, StoreError> {
        let mut tx = self.begin().await?;
        let base_rows = tx
            .fetch(self.get_fact_stmt(ctx, id))
            .await
            .map_err(map_sql_err)?;
        let Some(base_row) = base_rows.first() else {
            return Err(StoreError::NotFound);
        };
        let base = row_to_fact(base_row)?;
        owner_gate(ctx, &base.owner.user)?;
        let mut embeddings_removed: u32 = u32::from(base_row.boolean("has_embedding")?);

        // The consolidated insights derived from this fact, under the same read filter (served by
        // `mem_fact_derived_from`).
        // The owner rule (SA-AUTHZ-07) removes the caller's own. A teammate's summary of this fact
        // stays, with an ended origin.
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        let p_base = st.text(id);
        let read = fact_read_filter(&mut st, ctx);
        st.sql(&format!(
            "SELECT {cols} FROM mem_fact f WHERE f.id IN \
             (SELECT d.fact_id FROM mem_fact_derived d WHERE d.derived_from = {p_base}) \
             AND ({read}) ORDER BY f.id"
        ));
        let mut derived_removed = Vec::new();
        let mut removed_facts = vec![base.clone()];
        for row in tx.fetch(st).await.map_err(map_sql_err)? {
            if !owner_view(ctx, &row.text("owner_user")?) {
                continue;
            }
            if row.boolean("has_embedding")? {
                embeddings_removed += 1;
            }
            derived_removed.push(row.text("id")?);
            removed_facts.push(row_to_fact(&row)?);
        }

        let mut removed_ids = Vec::with_capacity(derived_removed.len() + 1);
        removed_ids.push(base.id.clone());
        removed_ids.extend(derived_removed.iter().cloned());
        self.drop_jobs_of(&mut tx, &ctx.tenant, &removed_ids)
            .await?;

        // Step 13′: the link rows of the removal set, and the relationships left with no asserting
        // fact and no `replaces`.
        // spec: components/memory-store#SA-GRAPH-04
        let orphans = self
            .orphaned_by(&mut tx, &ctx.tenant, &removed_ids, &Default::default())
            .await?;
        // Step 13‴: what only the removal set used, read while its rows are there.
        // spec: phase-1-analysis#SA-DELETE-02
        let (entities_removed, sources_removed) = self
            .unused_after(&mut tx, &ctx.tenant, &removed_facts, &removed_ids, &orphans)
            .await?;
        self.delete_links(&mut tx, &ctx.tenant, &removed_ids, &orphans)
            .await?;

        // The junction rows go with their fact (the foreign keys cascade where the host enables
        // them; deleting them here makes erasure independent of that setting).
        for table in ["mem_fact_entity", "mem_fact_derived"] {
            let mut st = self.stmt();
            let in_ids = st.in_list("fact_id", &removed_ids);
            st.sql(&format!("DELETE FROM {table} WHERE {in_ids}"));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        let mut st = self.stmt();
        let in_ids = st.in_list("id", &removed_ids);
        let p_t = st.text(&ctx.tenant);
        st.sql(&format!(
            "DELETE FROM mem_fact WHERE {in_ids} AND tenant = {p_t}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        self.delete_unused(&mut tx, &ctx.tenant, &entities_removed, &sources_removed)
            .await?;

        // Verify every collected id is gone before committing; a partial delete is never reported
        // complete, and dropping the transaction leaves every row as it was (SA-DELETE-01).
        let mut st = self.stmt();
        let in_ids = st.in_list("f.id", &removed_ids);
        let read = fact_read_filter(&mut st, ctx);
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM mem_fact f WHERE {in_ids} AND ({read})"
        ));
        let still = count_of(&tx.fetch(st).await.map_err(map_sql_err)?)?;
        if still > 0 {
            let expected = removed_ids.len() as u32;
            return Err(StoreError::PartialDelete {
                removed: expected.saturating_sub(u32::try_from(still).unwrap_or(u32::MAX)),
                expected,
            });
        }
        tx.commit().await.map_err(map_sql_err)?;
        self.embeddings.forget(&ctx.tenant, &removed_ids);
        self.sweep_summaries(ctx, &removed_ids).await?;

        let mut sorted = removed_ids;
        sorted.extend(orphans.iter().cloned());
        sorted.extend(entities_removed.iter().cloned());
        sorted.extend(sources_removed.iter().cloned());
        sorted.sort();
        let digest = sha256_hex(&sorted);
        tracing::debug!(
            target: "memory",
            derived_removed_count = derived_removed.len(),
            embeddings_removed,
            "store.hard_delete"
        );
        Ok(DeletionProof {
            relationships_removed: orphans,
            entities_removed,
            sources_removed,
            deleted_at: Utc::now(),
            record_id: id.to_string(),
            derived_removed,
            embeddings_removed,
            digest,
        })
    }

    async fn sweep_summaries(
        &self,
        ctx: &ScopeContext,
        gone: &[String],
    ) -> Result<u32, StoreError> {
        if gone.is_empty() {
            return Ok(0);
        }
        let mut tx = self.begin().await?;
        self.drop_jobs_of(&mut tx, &ctx.tenant, gone).await?;
        tx.commit().await.map_err(map_sql_err)?;
        let everyone = is_maintenance_scope(ctx);
        let (dead_user, dead_team, dead_list) = match self.dialect {
            SqlDialect::Sqlite => (
                "json_extract(scope, '$.user')",
                "json_extract(scope, '$.team')",
                "json_each(payload, '$.derived_from') j",
            ),
            SqlDialect::Postgres => (
                "scope->>'user'",
                "scope->>'team'",
                "jsonb_array_elements_text(payload->'derived_from') j(value)",
            ),
        };
        let held_list = match self.dialect {
            SqlDialect::Sqlite => "json_each(derived_from) j",
            SqlDialect::Postgres => "jsonb_array_elements_text(derived_from) j(value)",
        };
        for (table, list, owner, team) in [
            ("mem_quarantine", held_list, "owner_user", "owner_team"),
            ("mem_dead_letter", dead_list, dead_user, dead_team),
        ] {
            let mut st = self.stmt();
            let p_t = st.text(&ctx.tenant);
            let named = st.in_list("j.value", gone);
            let mine = if everyone {
                let teams = st.in_list(team, &ctx.teams);
                format!(" AND {teams}")
            } else {
                let p_u = st.text(&ctx.user);
                format!(" AND {owner} = {p_u}")
            };
            st.sql(&format!(
                "DELETE FROM {table} WHERE tenant = {p_t}{mine} \
                 AND EXISTS (SELECT 1 FROM {list} WHERE {named})"
            ));
            self.pool.execute(st).await.map_err(map_sql_err)?;
        }
        let mut st = self.stmt();
        let named = st.in_list("d.derived_from", gone);
        let read = fact_read_filter(&mut st, ctx);
        st.sql(&format!(
            "SELECT f.id AS id, f.owner_user AS owner_user, f.owner_team AS owner_team \
             FROM mem_fact f WHERE f.id IN \
             (SELECT d.fact_id FROM mem_fact_derived d WHERE {named}) AND ({read})"
        ));
        let mut swept = 0;
        for row in self.fetch(st).await? {
            if !owner_view(ctx, &row.text("owner_user")?) {
                continue;
            }
            if everyone
                && !ctx
                    .teams
                    .contains(&row.opt_text("owner_team")?.unwrap_or_default())
            {
                continue;
            }
            match self.hard_delete(ctx, &row.text("id")?).await {
                Ok(_) => swept += 1,
                Err(StoreError::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(swept)
    }

    async fn put_entity(&self, e: &Entity) -> Result<(), StoreError> {
        if e.canonical_name.is_empty() || e.canonical_name.chars().count() > 512 {
            return Err(StoreError::Validation("canonical_name length".into()));
        }
        validate_collection(&e.collection)?;
        let mut st = self.stmt();
        let vals = [
            st.text(&e.id),
            st.text(&e.owner.tenant),
            st.opt_text(e.owner.team.clone()),
            st.text(&e.owner.user),
            st.text(&e.canonical_name),
            st.json(&Json::from(e.public_aliases())),
            st.text(&e.collection),
            st.json(&Json::from(e.private_aliases.clone())),
            st.json(&folders_json(e)?),
        ]
        .join(", ");
        st.sql(&format!(
            "INSERT INTO mem_entity (id, tenant, owner_team, owner_user, canonical_name, aliases, \
             collection, private_aliases, alias_folders) VALUES ({vals}) \
             ON CONFLICT (id) DO UPDATE SET canonical_name = excluded.canonical_name, \
             aliases = excluded.aliases, collection = excluded.collection, \
             private_aliases = excluded.private_aliases, alias_folders = excluded.alias_folders \
             WHERE mem_entity.tenant = excluded.tenant \
             AND mem_entity.owner_user = excluded.owner_user"
        ));
        // Step 7′: the entity's normalised names are written with the row.
        // spec: components/memory-store#SA-GRAPH-01
        let mut tx = self.begin().await?;
        if tx.execute(st).await.map_err(map_sql_err)? == 0 {
            return Err(StoreError::ScopeForbidden);
        }
        self.write_entity_names(&mut tx, e).await?;
        tx.commit().await.map_err(map_sql_err)?;
        Ok(())
    }

    async fn get_entity(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Entity>, StoreError> {
        let mut st = self.stmt();
        let cols = entity_cols(self.dialect);
        let p_id = st.text(id);
        let read = entity_read_filter(&mut st, ctx, "e");
        st.sql(&format!(
            "SELECT {cols} FROM mem_entity e WHERE e.id = {p_id} AND ({read})"
        ));
        self.fetch(st)
            .await?
            .first()
            .map(|r| entity_for(ctx, r))
            .transpose()
    }

    async fn find_entity_by_name(
        &self,
        ctx: &ScopeContext,
        name: &str,
    ) -> Result<Vec<Entity>, StoreError> {
        let mut st = self.stmt();
        let cols = entity_cols(self.dialect);
        let p_name = st.text(name);
        let in_list = |col: &str| match self.dialect {
            SqlDialect::Sqlite => {
                format!("EXISTS (SELECT 1 FROM json_each({col}) WHERE value = {p_name})")
            }
            SqlDialect::Postgres => format!("{col} @> jsonb_build_array({p_name}::text)"),
        };
        let in_aliases = in_list("e.aliases");
        let in_private = in_list("e.private_aliases");
        let p_user = st.text(&ctx.user);
        let read = entity_read_filter(&mut st, ctx, "e");
        st.sql(&format!(
            "SELECT {cols} FROM mem_entity e \
             WHERE (e.canonical_name = {p_name} OR {in_aliases} \
                    OR (e.owner_user = {p_user} AND {in_private})) AND ({read})"
        ));
        self.fetch(st)
            .await?
            .iter()
            .map(|r| entity_for(ctx, r))
            .collect()
    }

    async fn merge_entities(
        &self,
        ctx: &ScopeContext,
        keep_id: &str,
        merge_id: &str,
    ) -> Result<(), StoreError> {
        let keep = match self.get_entity(ctx, keep_id).await? {
            Some(e) => e,
            None => return Err(StoreError::NotFound),
        };
        let merge = match self.get_entity(ctx, merge_id).await? {
            Some(e) => e,
            None => return Err(StoreError::NotFound),
        };
        owner_gate(ctx, &keep.owner.user)?;
        owner_gate(ctx, &merge.owner.user)?;

        // Fold merge's canonical name + aliases into keep's aliases (deduplicated): a name only private
        // facts carry stays private on `keep`.
        let mut named = [false; 3];
        let asks = [(keep_id, "="), (keep_id, "<>"), (merge_id, "<>")];
        for (flag, (id, op)) in named.iter_mut().zip(asks) {
            let mut st = self.stmt();
            let (p_t, p_id) = (st.text(&ctx.tenant), st.text(id));
            st.sql(&format!(
                "SELECT COUNT(*) AS n FROM mem_fact f WHERE f.tenant = {p_t} \
                 AND f.visibility {op} 'user-private' AND f.id IN \
                 (SELECT fe.fact_id FROM mem_fact_entity fe WHERE fe.entity_id = {p_id})"
            ));
            *flag = count_of(&self.fetch(st).await?)? > 0;
        }
        let [keep_private, keep_shared, merge_shared] = named;
        crate::store::graph::refuse_name_exposure(keep_private, keep_shared, merge_shared)?;
        let folded = crate::store::graph::fold_aliases(&keep, &merge, merge_shared);

        let mut tx = self.begin().await?;
        // Repoint every fact naming merge_id: its `entities` array and its junction rows.
        let mut repointed: Vec<String> = Vec::new();
        let mut st = self.stmt();
        let entities = json_col(self.dialect, "f.entities", "entities");
        let (p_t, p_mid) = (st.text(&ctx.tenant), st.text(merge_id));
        st.sql(&format!(
            "SELECT f.id AS id, {entities} FROM mem_fact f WHERE f.tenant = {p_t} AND f.id IN \
             (SELECT fe.fact_id FROM mem_fact_entity fe WHERE fe.entity_id = {p_mid})"
        ));
        for row in tx.fetch(st).await.map_err(map_sql_err)? {
            let fact_id = row.text("id")?;
            repointed.push(fact_id.clone());
            // `array::union(array::complement(entities, [merge]), [keep])`: merge removed, keep
            // present once, order otherwise kept.
            let mut list: Vec<String> = Vec::new();
            for e in string_list(row.json("entities")?, "entities")? {
                if e != merge_id && !list.contains(&e) {
                    list.push(e);
                }
            }
            if !list.iter().any(|e| e == keep_id) {
                list.push(keep_id.to_string());
            }
            let mut st = self.stmt();
            let (p_list, p_id) = (st.json(&Json::from(list)), st.text(&fact_id));
            st.sql(&format!(
                "UPDATE mem_fact SET entities = {p_list} WHERE id = {p_id}"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
            let mut st = self.stmt();
            let (p_id, p_mid) = (st.text(&fact_id), st.text(merge_id));
            st.sql(&format!(
                "DELETE FROM mem_fact_entity WHERE fact_id = {p_id} AND entity_id = {p_mid}"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
            let mut st = self.stmt();
            let (p_id, p_kid) = (st.text(&fact_id), st.text(keep_id));
            st.sql(&format!(
                "INSERT INTO mem_fact_entity (fact_id, entity_id) VALUES ({p_id}, {p_kid}) \
                 ON CONFLICT DO NOTHING"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        for col in ["from_ent", "to_ent"] {
            let mut st = self.stmt();
            let (p_kid, p_t, p_mid) = (st.text(keep_id), st.text(&ctx.tenant), st.text(merge_id));
            st.sql(&format!(
                "UPDATE mem_relationship SET {col} = {p_kid} \
                 WHERE tenant = {p_t} AND {col} = {p_mid}"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        // Step 19d′: of two live relationships that became identical, the earlier stays and takes the
        // other's fact rows; a relationship that became a self-loop goes.
        // spec: components/memory-store#SA-GRAPH-04
        let mut st = self.stmt();
        let p_t = st.text(&ctx.tenant);
        let ends = [keep_id.to_string()];
        let from = st.in_list("r.from_ent", &ends);
        let to = st.in_list("r.to_ent", &ends);
        st.sql(&format!(
            "SELECT {RELATIONSHIP_COLS} FROM mem_relationship r WHERE r.tenant = {p_t} AND ({from} OR {to})"
        ));
        let rels: Vec<Relationship> = tx
            .fetch(st)
            .await
            .map_err(map_sql_err)?
            .iter()
            .map(row_to_relationship)
            .collect::<Result<_, _>>()?;
        let (drop, moves) = crate::store::graph::merge_plan(&rels, keep_id, merge_id);
        // Returning here drops the transaction, so nothing above stays.
        crate::store::graph::refuse_merge_of_others(ctx, &rels, &drop)?;
        for (from_rel, to_rel) in &moves {
            let mut st = self.stmt();
            let (p_to, p_from) = (st.text(to_rel), st.text(from_rel));
            st.sql(&format!(
                "INSERT INTO mem_relationship_fact (relationship_id, fact_id, role, at) \
                 SELECT {p_to}, rf.fact_id, rf.role, rf.at FROM mem_relationship_fact rf \
                 WHERE rf.relationship_id = {p_from} ON CONFLICT DO NOTHING"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        self.delete_links(&mut tx, &ctx.tenant, &[], &drop).await?;

        self.write_aliases(&mut tx, &folded).await?;
        let mut st = self.stmt();
        let p_mid = st.text(merge_id);
        st.sql(&format!(
            "DELETE FROM mem_entity_name WHERE entity_id = {p_mid}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        let mut st = self.stmt();
        let (p_mid, p_t) = (st.text(merge_id), st.text(&ctx.tenant));
        st.sql(&format!(
            "DELETE FROM mem_entity WHERE id = {p_mid} AND tenant = {p_t}"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        tx.commit().await.map_err(map_sql_err)?;
        // `entities` is a column of the search cache.
        self.embeddings.forget(&ctx.tenant, &repointed);
        Ok(())
    }

    async fn put_relationship(&self, r: &Relationship) -> Result<(), StoreError> {
        if r.kind.is_empty() || r.kind.chars().count() > 128 {
            return Err(StoreError::Validation("kind length".into()));
        }
        if !(0.0..=1.0).contains(&r.confidence) {
            return Err(StoreError::Validation("confidence out of range".into()));
        }
        validate_collection(&r.collection)?;
        crate::store::graph::validate_relationship_graph_fields(&r.kind, r.replaces.as_deref())?;
        let mut st = self.stmt();
        let vals = [
            st.text(&r.id),
            st.text(&r.owner.tenant),
            st.opt_text(r.owner.team.clone()),
            st.text(&r.owner.user),
            st.text(&r.kind),
            st.text(&r.from),
            st.text(&r.to),
            st.ts(r.valid_from),
            st.opt_ts(r.valid_to),
            st.ts(r.ingested_at),
            st.float(r.confidence),
            st.opt_text(r.source_id.clone()),
            st.text(&r.collection),
            st.opt_text(r.replaces.clone()),
            st.boolean(r.owner_only),
            st.opt_text(r.author.clone()),
        ]
        .join(", ");
        st.sql(&format!(
            "INSERT INTO mem_relationship (id, tenant, owner_team, owner_user, kind, from_ent, \
             to_ent, valid_from, valid_to, ingested_at, confidence, source_id, collection, replaces, \
             owner_only, author) VALUES ({vals}) \
             ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, from_ent = excluded.from_ent, \
             to_ent = excluded.to_ent, valid_from = excluded.valid_from, \
             valid_to = excluded.valid_to, ingested_at = excluded.ingested_at, \
             confidence = excluded.confidence, source_id = excluded.source_id, \
             collection = excluded.collection, replaces = excluded.replaces, \
             owner_only = excluded.owner_only \
             WHERE mem_relationship.tenant = excluded.tenant \
             AND mem_relationship.owner_user = excluded.owner_user"
        ));
        if self.execute(st).await? == 0 {
            return Err(StoreError::ScopeForbidden);
        }
        Ok(())
    }

    async fn get_relationship(
        &self,
        ctx: &ScopeContext,
        id: &str,
    ) -> Result<Option<Relationship>, StoreError> {
        let mut st = self.stmt();
        let p_id = st.text(id);
        let read = rel_read_filter(&mut st, ctx, "r");
        st.sql(&format!(
            "SELECT {RELATIONSHIP_COLS} FROM mem_relationship r WHERE r.id = {p_id} AND ({read})"
        ));
        self.fetch(st)
            .await?
            .first()
            .map(row_to_relationship)
            .transpose()
    }

    async fn end_relationship_validity(
        &self,
        ctx: &ScopeContext,
        id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let Some(rel) = self.get_relationship(ctx, id).await? else {
            return Err(StoreError::NotFound);
        };
        owner_gate(ctx, &rel.owner.user)?;
        let mut st = self.stmt();
        let (p_at, p_id, p_t) = (st.ts(at), st.text(id), st.text(&ctx.tenant));
        st.sql(&format!(
            "UPDATE mem_relationship SET valid_to = {p_at} \
             WHERE id = {p_id} AND tenant = {p_t} AND valid_to IS NULL"
        ));
        self.execute(st).await?;
        Ok(())
    }

    async fn put_source(&self, s: &Source) -> Result<(), StoreError> {
        if !(0.0..=1.0).contains(&s.trust_signal) {
            return Err(StoreError::Validation("trust_signal out of range".into()));
        }
        validate_collection(&s.collection)?;
        let mut st = self.stmt();
        let vals = [
            st.text(&s.id),
            st.text(&s.owner.tenant),
            st.opt_text(s.owner.team.clone()),
            st.text(&s.owner.user),
            st.text(&s.origin_ref),
            st.opt_text(s.modification_marker.clone()),
            st.float(s.trust_signal),
            st.text(&s.collection),
        ]
        .join(", ");
        st.sql(&format!(
            "INSERT INTO mem_source (id, tenant, owner_team, owner_user, origin_ref, \
             modification_marker, trust_signal, collection) VALUES ({vals}) \
             ON CONFLICT (id) DO UPDATE SET origin_ref = excluded.origin_ref, \
             modification_marker = excluded.modification_marker, \
             trust_signal = excluded.trust_signal, collection = excluded.collection \
             WHERE mem_source.tenant = excluded.tenant \
             AND mem_source.owner_user = excluded.owner_user"
        ));
        if self.execute(st).await? == 0 {
            return Err(StoreError::ScopeForbidden);
        }
        Ok(())
    }

    async fn get_source(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Source>, StoreError> {
        let mut st = self.stmt();
        let p_id = st.text(id);
        let read = novis_filter(&mut st, ctx, "s");
        st.sql(&format!(
            "SELECT {SOURCE_COLS} FROM mem_source s WHERE s.id = {p_id} AND ({read})"
        ));
        self.fetch(st).await?.first().map(row_to_source).transpose()
    }

    async fn record_owner(&self, tenant: &str, id: &str) -> Result<Option<ScopeRef>, StoreError> {
        let _ = tenant; // Ids are unique across tenants here: a row of another tenant is refused too.
        let Some(table) = owned_table(id) else {
            return Ok(None);
        };
        let mut st = self.stmt();
        let p_id = st.text(id);
        st.sql(&format!(
            "SELECT tenant, owner_team, owner_user FROM {table} WHERE id = {p_id}"
        ));
        self.fetch(st).await?.first().map(owner_of).transpose()
    }

    async fn append_audit(&self, e: &AuditEntry) -> Result<(), StoreError> {
        let scope = serde_json::to_value(&e.scope)
            .map_err(|err| StoreError::Internal(format!("encode audit scope: {err}")))?;
        let mut st = self.stmt();
        let vals = [
            st.text(&e.id),
            st.text(&e.tenant),
            st.text(&e.subject),
            st.text(&e.operation),
            st.json(&scope),
            st.text(&e.outcome),
            st.text(&e.token_jti),
            st.text(&e.correlation_id),
            st.opt_text(e.client.clone()),
            st.opt_text(e.fact_class.clone()),
            st.ts(e.at),
        ]
        .join(", ");
        st.sql(&format!(
            "INSERT INTO mem_audit_log (id, tenant, subject, operation, scope, outcome, token_jti, \
             correlation_id, client, fact_class, at) VALUES ({vals})"
        ));
        self.execute(st).await?;
        Ok(())
    }

    async fn list_tenants(&self) -> Result<Vec<String>, StoreError> {
        let mut st = self.stmt();
        let union = ERASURE_SET
            .iter()
            .map(|t| format!("SELECT tenant FROM {t}"))
            .collect::<Vec<_>>()
            .join(" UNION ");
        st.sql(&format!(
            "SELECT DISTINCT tenant FROM ({union}) tenants ORDER BY tenant"
        ));
        self.fetch(st)
            .await?
            .iter()
            .map(|r| r.text("tenant"))
            .collect()
    }

    async fn scan_recent_episodes(
        &self,
        ctx: &ScopeContext,
        since: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError> {
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        let (p_t, p_since, p_l) = (st.text(&ctx.tenant), st.ts(since), st.int(i64::from(limit)));
        st.sql(&format!(
            "SELECT {cols} FROM mem_fact f WHERE f.tenant = {p_t} AND f.valid_to IS NULL \
             AND f.memory_class = 'episodic' AND f.ingested_at >= {p_since} LIMIT {p_l}"
        ));
        self.fetch(st).await?.iter().map(row_to_fact).collect()
    }

    async fn scan_contradiction_candidates(
        &self,
        ctx: &ScopeContext,
        limit: u32,
    ) -> Result<Vec<(Fact, Fact)>, StoreError> {
        // Currently-valid facts; pair those sharing ≥1 entity, client-side over a bounded window.
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        let (p_t, p_l) = (
            st.text(&ctx.tenant),
            st.int(i64::from(limit.saturating_mul(2).max(2))),
        );
        st.sql(&format!(
            "SELECT {cols} FROM mem_fact f WHERE f.tenant = {p_t} AND f.valid_to IS NULL \
             LIMIT {p_l}"
        ));
        let facts: Vec<Fact> = self
            .fetch(st)
            .await?
            .iter()
            .map(row_to_fact)
            .collect::<Result<_, _>>()?;
        let mut pairs = Vec::new();
        for i in 0..facts.len() {
            for j in (i + 1)..facts.len() {
                if facts[i]
                    .entities
                    .iter()
                    .any(|e| facts[j].entities.contains(e))
                {
                    pairs.push((facts[i].clone(), facts[j].clone()));
                    if pairs.len() as u32 >= limit {
                        return Ok(pairs);
                    }
                }
            }
        }
        Ok(pairs)
    }

    async fn scan_decay_candidates(
        &self,
        ctx: &ScopeContext,
        salience_floor: f64,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError> {
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        let (p_t, p_floor, p_l) = (
            st.text(&ctx.tenant),
            st.float(salience_floor),
            st.int(i64::from(limit)),
        );
        // Never-recalled facts first, oldest first (C1 step 17).
        st.sql(&format!(
            "SELECT {cols} FROM mem_fact f WHERE f.tenant = {p_t} AND f.valid_to IS NULL \
             AND f.salience < {p_floor} \
             ORDER BY f.last_recalled_at ASC NULLS FIRST, f.ingested_at ASC LIMIT {p_l}"
        ));
        self.fetch(st).await?.iter().map(row_to_fact).collect()
    }

    async fn scan_reembed_candidates(
        &self,
        ctx: &ScopeContext,
        current_model_version: &str,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError> {
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        let (p_t, p_cur, p_l) = (
            st.text(&ctx.tenant),
            st.text(current_model_version),
            st.int(i64::from(limit)),
        );
        st.sql(&format!(
            "SELECT {cols} FROM mem_fact f WHERE f.tenant = {p_t} AND f.valid_to IS NULL \
             AND (f.embedding_model IS NULL OR f.embedding_model <> {p_cur} \
                  OR f.embedding IS NULL) LIMIT {p_l}"
        ));
        self.fetch(st).await?.iter().map(row_to_fact).collect()
    }

    async fn update_fact_maintenance_fields(
        &self,
        ctx: &ScopeContext,
        f: &Fact,
    ) -> Result<(), StoreError> {
        if self.get_fact(ctx, &f.id).await?.is_none() {
            return Err(StoreError::NotFound);
        }
        let mut st = self.stmt();
        let (p_c, p_s, p_st, p_lr, p_sup, p_supby, p_id, p_t) = (
            st.float(f.confidence),
            st.float(f.salience),
            st.float(f.stability),
            st.opt_ts(f.last_recalled_at),
            st.opt_text(f.supersedes.clone()),
            st.opt_text(f.superseded_by.clone()),
            st.text(&f.id),
            st.text(&ctx.tenant),
        );
        st.sql(&format!(
            "UPDATE mem_fact SET confidence = {p_c}, salience = {p_s}, stability = {p_st}, \
             last_recalled_at = {p_lr}, supersedes = {p_sup}, superseded_by = {p_supby} \
             WHERE id = {p_id} AND tenant = {p_t}"
        ));
        self.execute(st).await?;
        Ok(())
    }

    async fn set_fact_embedding(
        &self,
        ctx: &ScopeContext,
        fact_id: &str,
        vector: &[f32],
        model_version: &str,
    ) -> Result<(), StoreError> {
        if vector.len() as u32 != self.embed_dim {
            return Err(StoreError::Validation(format!(
                "embedding dimension {} != MEMORY_EMBED_DIM {}",
                vector.len(),
                self.embed_dim
            )));
        }
        let mut st = self.stmt();
        let (p_v, p_m, p_id, p_t) = (
            st.bytes(Some(encode_embedding(vector))),
            st.text(model_version),
            st.text(fact_id),
            st.text(&ctx.tenant),
        );
        st.sql(&format!(
            "UPDATE mem_fact SET embedding = {p_v}, embedding_model = {p_m} \
             WHERE id = {p_id} AND tenant = {p_t}"
        ));
        self.execute(st).await?;
        self.embeddings.forget(&ctx.tenant, &[fact_id.to_string()]);
        Ok(())
    }

    async fn ensure_tenant_namespace(&self, _tenant: &str) -> Result<(), StoreError> {
        // Nothing to provision: the host applies §B once and every tenant shares its tables.
        Ok(())
    }

    async fn erase_tenant(&self, tenant: &str) -> Result<TenantErasureProof, StoreError> {
        let mut tx = self.begin().await?;
        let mut removed: BTreeMap<String, u64> = BTreeMap::new();
        let mut total_removed: u64 = 0;
        for table in ERASURE_SET {
            let n = self.count_tenant_rows(&mut tx, table, tenant).await?;
            if *table == "mem_fact" {
                for junction in ["mem_fact_entity", "mem_fact_derived"] {
                    let mut st = self.stmt();
                    let p = st.text(tenant);
                    st.sql(&format!(
                        "DELETE FROM {junction} WHERE fact_id IN \
                         (SELECT id FROM mem_fact WHERE tenant = {p})"
                    ));
                    tx.execute(st).await.map_err(map_sql_err)?;
                }
                // `mem_relationship_fact` carries no tenant: its rows go with the tenant's facts and
                // relationships.
                let mut st = self.stmt();
                let p = st.text(tenant);
                st.sql(&format!(
                    "DELETE FROM mem_relationship_fact WHERE fact_id IN \
                     (SELECT id FROM mem_fact WHERE tenant = {p}) OR relationship_id IN \
                     (SELECT id FROM mem_relationship WHERE tenant = {p})"
                ));
                tx.execute(st).await.map_err(map_sql_err)?;
            }
            let mut st = self.stmt();
            let p = st.text(tenant);
            st.sql(&format!("DELETE FROM {table} WHERE tenant = {p}"));
            tx.execute(st).await.map_err(map_sql_err)?;
            total_removed += n;
            removed.insert((*table).to_string(), n);
        }

        // Verification: re-count on the same predicate before committing; anything left is a
        // partial erasure and is never reported as success (SA-DELETE-01 on the tenant path).
        let mut remaining: u64 = 0;
        for table in ERASURE_SET {
            remaining += self.count_tenant_rows(&mut tx, table, tenant).await?;
        }
        if remaining > 0 {
            let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
            return Err(StoreError::PartialDelete {
                removed: clamp(total_removed.saturating_sub(remaining)),
                expected: clamp(total_removed),
            });
        }
        tx.commit().await.map_err(map_sql_err)?;
        self.embeddings.forget_tenant(tenant);

        let digest = sha256_hex(&erasure_digest_input(tenant, SHARED_NAMESPACE, &removed));
        tracing::warn!(
            target: "memory",
            total_removed,
            tenant_present = crate::obs::log::tenant_present(Some(tenant)),
            "store.erase_tenant"
        );
        Ok(TenantErasureProof {
            erased_at: Utc::now(),
            tenant: tenant.to_string(),
            namespace: SHARED_NAMESPACE.to_string(),
            removed,
            total_removed,
            digest,
        })
    }

    async fn drop_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError> {
        // The tables hold every tenant, so an unbounded drop would erase co-resident tenants; refuse,
        // as ADR-018 requires wherever the namespace is not exclusive.
        tracing::warn!(target: "memory", "store.drop_ns.refused");
        Err(StoreError::NamespaceNotExclusive {
            tenant: tenant.to_string(),
            namespace: SHARED_NAMESPACE.to_string(),
        })
    }

    async fn ready(&self) -> Result<(), StoreError> {
        // The connection answers, and no stored embedding has a length other than `embed_dim`
        // (the SQL counterpart of "vector-index dim == MEMORY_EMBED_DIM").
        let mut st = self.stmt();
        let len = match self.dialect {
            SqlDialect::Sqlite => "length",
            SqlDialect::Postgres => "octet_length",
        };
        let p_bytes = st.int(i64::from(self.embed_dim) * 4);
        st.sql(&format!(
            "SELECT f.id AS id FROM mem_fact f \
             WHERE f.embedding IS NOT NULL AND {len}(f.embedding) <> {p_bytes} LIMIT 1"
        ));
        if let Some(row) = self.fetch(st).await?.first() {
            let raw = row.text("id")?;
            return Err(StoreError::Internal(format!(
                "stored embedding dimension != MEMORY_EMBED_DIM {} (fact {raw})",
                self.embed_dim
            )));
        }
        Ok(())
    }

    async fn list_facts_page(
        &self,
        ctx: &ScopeContext,
        after: Option<(DateTime<Utc>, String)>,
        include_retired: bool,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError> {
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        let read = fact_read_filter(&mut st, ctx);
        let validity = if include_retired {
            ""
        } else {
            " AND f.valid_to IS NULL"
        };
        let collate = id_collation(self.dialect);
        let keyset = match after {
            Some((at, id)) => {
                let (p_at, p_id) = (st.ts(at), st.text(id));
                format!(
                    " AND (f.ingested_at < {p_at} \
                     OR (f.ingested_at = {p_at} AND f.id{collate} < {p_id}))"
                )
            }
            None => String::new(),
        };
        let p_limit = st.int(i64::from(limit));
        st.sql(&format!(
            "SELECT {cols} FROM mem_fact f WHERE ({read}){validity}{keyset} \
             ORDER BY f.ingested_at DESC, f.id{collate} DESC LIMIT {p_limit}"
        ));
        self.fetch(st).await?.iter().map(row_to_fact).collect()
    }

    async fn export_facts_page(
        &self,
        ctx: &ScopeContext,
        after_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError> {
        let mut st = self.stmt();
        let cols = fact_cols(self.dialect);
        let read = fact_read_filter(&mut st, ctx);
        let collate = id_collation(self.dialect);
        let keyset = match after_id {
            Some(id) => {
                let p_id = st.text(id);
                format!(" AND f.id{collate} > {p_id}")
            }
            None => String::new(),
        };
        let p_limit = st.int(i64::from(limit));
        st.sql(&format!(
            "SELECT {cols} FROM mem_fact f WHERE ({read}){keyset} \
             ORDER BY f.id{collate} ASC LIMIT {p_limit}"
        ));
        self.fetch(st).await?.iter().map(row_to_fact).collect()
    }

    async fn relationships_among(
        &self,
        ctx: &ScopeContext,
        entity_ids: &[String],
    ) -> Result<Vec<Relationship>, StoreError> {
        if entity_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = self.stmt();
        let from = st.in_list("r.from_ent", entity_ids);
        let to = st.in_list("r.to_ent", entity_ids);
        let read = rel_read_filter(&mut st, ctx, "r");
        let collate = id_collation(self.dialect);
        st.sql(&format!(
            "SELECT {RELATIONSHIP_COLS} FROM mem_relationship r \
             WHERE {from} AND {to} AND ({read}) ORDER BY r.id{collate} ASC"
        ));
        self.fetch(st)
            .await?
            .iter()
            .map(row_to_relationship)
            .collect()
    }

    async fn set_pii_review(
        &self,
        ctx: &ScopeContext,
        id: &str,
        pii_review: bool,
    ) -> Result<(), StoreError> {
        let Some(fact) = self.get_fact(ctx, id).await? else {
            return Err(StoreError::NotFound);
        };
        owner_gate(ctx, &fact.owner.user)?;
        let mut st = self.stmt();
        let (p_v, p_id, p_t) = (st.boolean(pii_review), st.text(id), st.text(&ctx.tenant));
        st.sql(&format!(
            "UPDATE mem_fact SET pii_review = {p_v} WHERE id = {p_id} AND tenant = {p_t}"
        ));
        self.execute(st).await?;
        Ok(())
    }
}

/// `erase_scope` (ordin change M-6 c): every row of the tenant whose owner team is `team`, in one
/// transaction, re-counted before it commits.
#[async_trait]
impl ScopeErasure for SqlStorage {
    async fn erase_scope(
        &self,
        tenant: &str,
        team: &str,
    ) -> Result<TenantErasureProof, StoreError> {
        let mut tx = self.begin().await?;
        let mut st = self.stmt();
        let (p_t, p_team) = (st.text(tenant), st.text(team));
        st.sql(&format!(
            "SELECT id FROM mem_fact WHERE tenant = {p_t} AND owner_team = {p_team}"
        ));
        let erased = graph::texts(&tx.fetch(st).await.map_err(map_sql_err)?, "id")?;
        let mut removed: BTreeMap<String, u64> = BTreeMap::new();
        let mut total_removed: u64 = 0;
        for (table, sqlite_team, pg_team) in SCOPE_ERASURE_SET {
            let team_col = match self.dialect {
                SqlDialect::Sqlite => *sqlite_team,
                SqlDialect::Postgres => *pg_team,
            };
            let n = self
                .count_team_rows(&mut tx, table, team_col, tenant, team)
                .await?;
            if *table == "mem_fact" {
                for junction in ["mem_fact_entity", "mem_fact_derived"] {
                    let mut st = self.stmt();
                    let (p_t, p_team) = (st.text(tenant), st.text(team));
                    st.sql(&format!(
                        "DELETE FROM {junction} WHERE fact_id IN \
                         (SELECT id FROM mem_fact WHERE tenant = {p_t} AND owner_team = {p_team})"
                    ));
                    tx.execute(st).await.map_err(map_sql_err)?;
                }
                let mut st = self.stmt();
                let (p_t, p_team) = (st.text(tenant), st.text(team));
                st.sql(&format!(
                    "DELETE FROM mem_relationship_fact WHERE fact_id IN \
                     (SELECT id FROM mem_fact WHERE tenant = {p_t} AND owner_team = {p_team}) \
                     OR relationship_id IN \
                     (SELECT id FROM mem_relationship WHERE tenant = {p_t} AND owner_team = {p_team})"
                ));
                tx.execute(st).await.map_err(map_sql_err)?;
            }
            let mut st = self.stmt();
            let (p_t, p_team) = (st.text(tenant), st.text(team));
            st.sql(&format!(
                "DELETE FROM {table} WHERE tenant = {p_t} AND {team_col} = {p_team}"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
            total_removed += n;
            removed.insert((*table).to_string(), n);
        }

        // Verification: re-count on the same predicate before committing; anything left is a partial
        // erasure and is never reported as success (SA-DELETE-01 on the scope path).
        let mut remaining: u64 = 0;
        for (table, sqlite_team, pg_team) in SCOPE_ERASURE_SET {
            let team_col = match self.dialect {
                SqlDialect::Sqlite => *sqlite_team,
                SqlDialect::Postgres => *pg_team,
            };
            remaining += self
                .count_team_rows(&mut tx, table, team_col, tenant, team)
                .await?;
        }
        if remaining > 0 {
            let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
            return Err(StoreError::PartialDelete {
                removed: clamp(total_removed.saturating_sub(remaining)),
                expected: clamp(total_removed),
            });
        }
        self.drop_folder_aliases(&mut tx, tenant, team, None)
            .await?;
        tx.commit().await.map_err(map_sql_err)?;
        self.sweep_summaries(&crate::store::erasure_scope(tenant, team), &erased)
            .await?;
        self.embeddings.forget_tenant(tenant);

        let digest = sha256_hex(&erasure_digest_input(tenant, team, &removed));
        tracing::warn!(
            target: "memory",
            total_removed,
            tenant_present = crate::obs::log::tenant_present(Some(tenant)),
            "store.erase_scope"
        );
        Ok(TenantErasureProof {
            erased_at: Utc::now(),
            tenant: tenant.to_string(),
            namespace: team.to_string(),
            removed,
            total_removed,
            digest,
        })
    }
}

impl SqlStorage {
    /// The team's facts outside `collection` derived from its facts, at any depth.
    async fn summaries_of_collection(
        &self,
        tx: &mut Tx,
        tenant: &str,
        team: &str,
        collection: &str,
    ) -> Result<Vec<String>, StoreError> {
        let mut st = self.stmt();
        let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
        st.sql(&format!(
            "SELECT id FROM mem_fact WHERE tenant = {p_t} AND owner_team = {p_team} \
             AND collection = {p_c}"
        ));
        let rows = tx.fetch(st).await.map_err(map_sql_err)?;
        let mut seen: HashSet<String> = rows
            .iter()
            .map(|r| r.text("id"))
            .collect::<Result<_, _>>()?;
        let mut frontier: Vec<String> = seen.iter().cloned().collect();
        let mut out = Vec::new();
        while !frontier.is_empty() {
            let mut st = self.stmt();
            let in_d = st.in_list("d.derived_from", &frontier);
            let (p_t, p_team) = (st.text(tenant), st.text(team));
            st.sql(&format!(
                "SELECT DISTINCT f.id AS id FROM mem_fact_derived d \
                 JOIN mem_fact f ON f.id = d.fact_id \
                 WHERE {in_d} AND f.tenant = {p_t} AND f.owner_team = {p_team}"
            ));
            let rows = tx.fetch(st).await.map_err(map_sql_err)?;
            frontier = Vec::new();
            for r in rows {
                let id = r.text("id")?;
                if seen.insert(id.clone()) {
                    frontier.push(id.clone());
                    out.push(id);
                }
            }
        }
        Ok(out)
    }
}

/// `erase_collection` (ADR-020, step 20c; ordin change M-8 e), applied on `mem_*.collection` as the
/// SurrealDB store applies it (SA-COLL-05): one transaction, re-counted before it commits.
#[async_trait]
impl CollectionErasure for SqlStorage {
    // spec: components/memory-store#SA-COLL-05
    async fn erase_collection(
        &self,
        tenant: &str,
        team: &str,
        collection: &str,
    ) -> Result<TenantErasureProof, StoreError> {
        // Step 20c.1 — "range" selects VAL_OUT_OF_RANGE.
        if collection.is_empty() || !crate::types::domain::is_valid_collection(collection) {
            return Err(StoreError::Validation("collection out of range".into()));
        }
        let mut tx = self.begin().await?;
        let mut removed: BTreeMap<String, u64> = BTreeMap::new();

        // The writes still on their way go first, in the transaction's first statement: a worker either
        // committed its fact before this (and it is erased below) or finds its job gone and undoes the
        // write (`WritePipeline::process`). A held record and a failed job carry the same content.
        // The proof's `removed` gains `quarantine`, `work_job` and `dead_letter`. The digest's form is
        // unchanged.
        let (json_coll, json_team) = match self.dialect {
            SqlDialect::Sqlite => (
                "json_extract(payload, '$.collection')",
                "json_extract(scope, '$.team')",
            ),
            SqlDialect::Postgres => ("payload->>'collection'", "scope->>'team'"),
        };
        for (table, key, pred) in [
            (
                "mem_work_job",
                "work_job",
                format!("scope_team = {{team}} AND {json_coll} = {{c}}"),
            ),
            (
                "mem_dead_letter",
                "dead_letter",
                format!("{json_team} = {{team}} AND {json_coll} = {{c}}"),
            ),
            (
                "mem_quarantine",
                "quarantine",
                "owner_team = {team} AND collection = {c}".to_string(),
            ),
        ] {
            let mut st = self.stmt();
            let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
            let pred = pred.replace("{team}", &p_team).replace("{c}", &p_c);
            st.sql(&format!(
                "DELETE FROM {table} WHERE tenant = {p_t} AND {pred}"
            ));
            removed.insert(key.into(), tx.execute(st).await.map_err(map_sql_err)?);
        }

        // The team's summaries derived from them, at any depth and in any collection, go first with
        // their junction rows, whoever wrote them (a folder erasure is not owner-bound, unlike
        // `hard_delete`), and with the links left without an asserting fact (step 13′).
        let derived = self
            .summaries_of_collection(&mut tx, tenant, team, collection)
            .await?;
        let orphans = self
            .orphaned_by(&mut tx, tenant, &derived, &HashSet::new())
            .await?;
        self.delete_links(&mut tx, tenant, &derived, &orphans)
            .await?;
        for table in ["mem_fact_entity", "mem_fact_derived"] {
            let mut st = self.stmt();
            let in_f = st.in_list("fact_id", &derived);
            st.sql(&format!("DELETE FROM {table} WHERE {in_f}"));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        let mut st = self.stmt();
        let in_f = st.in_list("id", &derived);
        let p_t = st.text(tenant);
        st.sql(&format!(
            "DELETE FROM mem_fact WHERE {in_f} AND tenant = {p_t}"
        ));
        removed.insert("derived".into(), tx.execute(st).await.map_err(map_sql_err)?);

        // Step 20c.2 — the collection's facts (and their junction rows) first.
        let mut st = self.stmt();
        let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
        st.sql(&format!(
            "SELECT id FROM mem_fact WHERE tenant = {p_t} AND owner_team = {p_team} \
             AND collection = {p_c}"
        ));
        let mut erased = graph::texts(&tx.fetch(st).await.map_err(map_sql_err)?, "id")?;
        erased.extend(derived.iter().cloned());
        let mut st = self.stmt();
        let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
        st.sql(&format!(
            "DELETE FROM mem_relationship_fact WHERE fact_id IN (SELECT id FROM mem_fact \
             WHERE tenant = {p_t} AND owner_team = {p_team} AND collection = {p_c})"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        for junction in ["mem_fact_entity", "mem_fact_derived"] {
            let mut st = self.stmt();
            let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
            st.sql(&format!(
                "DELETE FROM {junction} WHERE fact_id IN (SELECT id FROM mem_fact \
                 WHERE tenant = {p_t} AND owner_team = {p_team} AND collection = {p_c})"
            ));
            tx.execute(st).await.map_err(map_sql_err)?;
        }
        let mut st = self.stmt();
        let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
        st.sql(&format!(
            "DELETE FROM mem_fact WHERE tenant = {p_t} AND owner_team = {p_team} \
             AND collection = {p_c}"
        ));
        removed.insert("fact".into(), tx.execute(st).await.map_err(map_sql_err)?);

        // The collection's entities no remaining fact of the team names. A relationship of the
        // collection, or of the team with an end among those entities, goes first, while the
        // entities it names can still be selected.
        let orphan_entities = |st: &mut Stmt| {
            let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
            format!(
                "SELECT e.id FROM mem_entity e WHERE e.tenant = {p_t} AND e.owner_team = {p_team} \
                 AND e.collection = {p_c} AND NOT EXISTS (SELECT 1 FROM mem_fact_entity fe \
                 JOIN mem_fact f ON f.id = fe.fact_id WHERE fe.entity_id = e.id \
                 AND f.tenant = {p_t} AND f.owner_team = {p_team})"
            )
        };
        let mut st = self.stmt();
        let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
        let gone_from = orphan_entities(&mut st);
        let gone_to = orphan_entities(&mut st);
        let rel_pred = format!(
            "tenant = {p_t} AND owner_team = {p_team} \
             AND (collection = {p_c} OR from_ent IN ({gone_from}) OR to_ent IN ({gone_to}))"
        );
        let mut rf = self.stmt();
        let (q_t, q_team, q_c) = (rf.text(tenant), rf.text(team), rf.text(collection));
        let rgone_from = orphan_entities(&mut rf);
        let rgone_to = orphan_entities(&mut rf);
        rf.sql(&format!(
            "DELETE FROM mem_relationship_fact WHERE relationship_id IN (SELECT id FROM mem_relationship \
             WHERE tenant = {q_t} AND owner_team = {q_team} AND (collection = {q_c} \
             OR from_ent IN ({rgone_from}) OR to_ent IN ({rgone_to})))"
        ));
        tx.execute(rf).await.map_err(map_sql_err)?;
        st.sql(&format!("DELETE FROM mem_relationship WHERE {rel_pred}"));
        let links_removed = tx.execute(st).await.map_err(map_sql_err)?;
        removed.insert("relationship".into(), links_removed + orphans.len() as u64);
        let mut st = self.stmt();
        let gone = orphan_entities(&mut st);
        st.sql(&format!(
            "DELETE FROM mem_entity_name WHERE entity_id IN ({gone})"
        ));
        tx.execute(st).await.map_err(map_sql_err)?;
        let mut st = self.stmt();
        let gone = orphan_entities(&mut st);
        st.sql(&format!("DELETE FROM mem_entity WHERE id IN ({gone})"));
        removed.insert("entity".into(), tx.execute(st).await.map_err(map_sql_err)?);

        self.drop_folder_aliases(&mut tx, tenant, team, Some(collection))
            .await?;

        // The collection's sources no remaining fact of the team cites.
        let mut st = self.stmt();
        let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
        st.sql(&format!(
            "DELETE FROM mem_source WHERE tenant = {p_t} AND owner_team = {p_team} \
             AND collection = {p_c} AND NOT EXISTS (SELECT 1 FROM mem_fact f \
             WHERE f.source_id = mem_source.id AND f.tenant = {p_t} AND f.owner_team = {p_team})"
        ));
        removed.insert("source".into(), tx.execute(st).await.map_err(map_sql_err)?);

        // Step 20c.3 — verify by re-count before committing.
        let mut st = self.stmt();
        let (p_t, p_team, p_c) = (st.text(tenant), st.text(team), st.text(collection));
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM mem_fact WHERE tenant = {p_t} AND owner_team = {p_team} \
             AND collection = {p_c}"
        ));
        let remaining = count_of(&tx.fetch(st).await.map_err(map_sql_err)?)?;
        if remaining > 0 {
            let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
            let facts = removed.get("fact").copied().unwrap_or(0);
            return Err(StoreError::PartialDelete {
                removed: clamp(facts),
                expected: clamp(facts + remaining),
            });
        }
        tx.commit().await.map_err(map_sql_err)?;
        self.embeddings.forget_tenant(tenant);
        self.sweep_summaries(&crate::store::erasure_scope(tenant, team), &erased)
            .await?;

        // Step 20c.4.
        let total_removed: u64 = removed.values().sum();
        let digest = sha256_hex(&crate::store::collection_erasure_digest_input(
            tenant,
            SHARED_NAMESPACE,
            team,
            collection,
            &removed,
        ));
        tracing::warn!(
            target: "memory",
            facts = removed.get("fact").copied().unwrap_or(0),
            entities = removed.get("entity").copied().unwrap_or(0),
            sources = removed.get("source").copied().unwrap_or(0),
            relationships = removed.get("relationship").copied().unwrap_or(0),
            held = removed.get("quarantine").copied().unwrap_or(0),
            jobs = removed.get("work_job").copied().unwrap_or(0),
            dead_letters = removed.get("dead_letter").copied().unwrap_or(0),
            digest = %digest,
            "store.erase_collection"
        );
        Ok(TenantErasureProof {
            erased_at: Utc::now(),
            tenant: tenant.to_string(),
            namespace: SHARED_NAMESPACE.to_string(),
            removed,
            total_removed,
            digest,
        })
    }
}

/// C8 idempotency records (SA-IDEM-01): one outcome per (tenant, user, route, key), replayed
/// verbatim until `expires_at`.
#[async_trait]
impl IdempotencyStore for SqlStorage {
    /// A non-expired outcome, or `None` (no row, or one whose `expires_at` is at or before now).
    async fn idempotency_get(
        &self,
        tenant: &str,
        user: &str,
        route: &str,
        key: &str,
    ) -> Result<Option<IdempotencyRecord>, StoreError> {
        let mut st = self.stmt();
        let body = json_col(self.dialect, "response_body", "response_body");
        let (p_t, p_u, p_r, p_k, p_now) = (
            st.text(tenant),
            st.text(user),
            st.text(route),
            st.text(key),
            st.ts(Utc::now()),
        );
        st.sql(&format!(
            "SELECT {body}, status_code, created_at, expires_at FROM mem_idempotency_record \
             WHERE tenant = {p_t} AND \"user\" = {p_u} AND route = {p_r} \
             AND idempotency_key = {p_k} AND expires_at > {p_now} LIMIT 1"
        ));
        let rows = self.fetch(st).await?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let status_code = u16::try_from(row.int("status_code")?)
            .map_err(|_| StoreError::Internal("idempotency_get: status_code".into()))?;
        Ok(Some(IdempotencyRecord {
            tenant: tenant.to_string(),
            user: user.to_string(),
            route: route.to_string(),
            idempotency_key: key.to_string(),
            response_body: row.json("response_body")?,
            status_code,
            created_at: row.ts("created_at")?,
            expires_at: row.ts("expires_at")?,
        }))
    }

    /// Upsert on the primary key, so a re-run after expiry overwrites the earlier outcome.
    async fn idempotency_put(&self, rec: &IdempotencyRecord) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let vals = [
            st.text(&rec.tenant),
            st.text(&rec.user),
            st.text(&rec.route),
            st.text(&rec.idempotency_key),
            st.json(&rec.response_body),
            st.int(i64::from(rec.status_code)),
            st.ts(rec.created_at),
            st.ts(rec.expires_at),
        ]
        .join(", ");
        st.sql(&format!(
            "INSERT INTO mem_idempotency_record (tenant, \"user\", route, idempotency_key, \
             response_body, status_code, created_at, expires_at) VALUES ({vals}) \
             ON CONFLICT (tenant, \"user\", route, idempotency_key) DO UPDATE SET \
             response_body = excluded.response_body, status_code = excluded.status_code, \
             created_at = excluded.created_at, expires_at = excluded.expires_at"
        ));
        self.execute(st).await?;
        Ok(())
    }

    /// Delete every record whose `expires_at` is at or before `now`, for every tenant.
    async fn idempotency_prune(&self, now: DateTime<Utc>) -> Result<u64, StoreError> {
        let mut st = self.stmt();
        let p_now = st.ts(now);
        st.sql(&format!(
            "DELETE FROM mem_idempotency_record WHERE expires_at <= {p_now}"
        ));
        self.execute(st).await
    }
}

/// C4 quarantine sink: gate-held writes land in `mem_quarantine`, never in `mem_fact`.
#[async_trait]
impl QuarantineStore for SqlStorage {
    /// Upsert by `idempotency_key` when present, so a replayed job lands on the row its first run
    /// wrote (keeping that row's id); without a key, by the record's own id.
    // §B's `mem_quarantine_idem` is unique on `idempotency_key` alone, across tenants, where the
    // SurrealDB store keeps one such index per tenant namespace. The stored key is scoped to the
    // owner, so one caller's key never reaches another's row.
    async fn put_quarantine(&self, rec: &QuarantineRecord) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let vals = [
            st.text(&rec.id),
            st.text(&rec.owner.tenant),
            st.opt_text(rec.owner.team.clone()),
            st.text(&rec.owner.user),
            st.json(&rec.content),
            st.json(&Json::from(rec.entities.clone())),
            st.opt_text(rec.source_id.clone()),
            st.text(memory_class_str(rec.memory_class)),
            st.opt_text(rec.fact_class.clone()),
            st.float(rec.trust_score),
            st.float(rec.confidence),
            st.float(rec.salience),
            st.boolean(rec.is_instruction_like),
            st.boolean(rec.pii_review),
            st.opt_text(
                rec.idempotency_key
                    .as_ref()
                    .map(|k| crate::store::scoped_quarantine_key(&rec.owner, k)),
            ),
            st.text(&rec.reason),
            st.ts(rec.quarantined_at),
            st.text(&rec.collection),
            st.json(&serde_json::to_value(&rec.entities_asserted).unwrap_or_default()),
            st.json(&serde_json::to_value(&rec.links_asserted).unwrap_or_default()),
            st.json(&Json::from(rec.derived_from.clone())),
            st.opt_text(rec.fact_id.clone()),
        ]
        .join(", ");
        let target = if rec.idempotency_key.is_some() {
            "(idempotency_key) WHERE idempotency_key IS NOT NULL"
        } else {
            "(id)"
        };
        st.sql(&format!(
            "INSERT INTO mem_quarantine (id, tenant, owner_team, owner_user, content, entities, \
             source_id, memory_class, fact_class, trust_score, confidence, salience, \
             is_instruction_like, pii_review, idempotency_key, reason, quarantined_at, collection, \
             entities_asserted, links_asserted, derived_from, fact_id) \
             VALUES ({vals}) \
             ON CONFLICT {target} DO UPDATE SET content = excluded.content, \
             entities = excluded.entities, source_id = excluded.source_id, \
             memory_class = excluded.memory_class, fact_class = excluded.fact_class, \
             trust_score = excluded.trust_score, confidence = excluded.confidence, \
             salience = excluded.salience, is_instruction_like = excluded.is_instruction_like, \
             pii_review = excluded.pii_review, reason = excluded.reason, \
             quarantined_at = excluded.quarantined_at, collection = excluded.collection, \
             entities_asserted = excluded.entities_asserted, links_asserted = excluded.links_asserted, \
             derived_from = excluded.derived_from, fact_id = excluded.fact_id \
             WHERE mem_quarantine.tenant = excluded.tenant \
             AND mem_quarantine.owner_user = excluded.owner_user"
        ));
        if self.execute(st).await? == 0 {
            return Err(StoreError::ScopeForbidden);
        }
        Ok(())
    }

    async fn drop_quarantine(&self, rec: &QuarantineRecord) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let (p_t, p_u, p_id) = (
            st.text(&rec.owner.tenant),
            st.text(&rec.owner.user),
            st.text(&rec.id),
        );
        let key = rec
            .idempotency_key
            .as_ref()
            .map(|k| crate::store::scoped_quarantine_key(&rec.owner, k));
        let p_key = st.opt_text(key);
        st.sql(&format!(
            "DELETE FROM mem_quarantine WHERE tenant = {p_t} AND owner_user = {p_u} \
             AND (id = {p_id} OR idempotency_key = {p_key})"
        ));
        self.execute(st).await?;
        Ok(())
    }

    async fn held_writes(
        &self,
        tenant: &str,
        team: Option<&str>,
        limit: u32,
    ) -> Result<Vec<QuarantineRecord>, StoreError> {
        let mut st = self.stmt();
        let cols = quarantine_cols(self.dialect);
        let p_t = st.text(tenant);
        let in_team = match team {
            Some(team) => format!("owner_team = {}", st.text(team)),
            None => "owner_team IS NULL".to_owned(),
        };
        let lim = st.int(i64::from(limit));
        st.sql(&format!(
            "SELECT {cols} FROM mem_quarantine WHERE tenant = {p_t} AND {in_team} \
             ORDER BY quarantined_at DESC, id LIMIT {lim}"
        ));
        self.fetch(st)
            .await?
            .iter()
            .map(row_to_quarantine)
            .collect()
    }

    async fn held_write(
        &self,
        tenant: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Option<QuarantineRecord>, StoreError> {
        let mut st = self.stmt();
        let cols = quarantine_cols(self.dialect);
        let (p_t, p_id) = (st.text(tenant), st.text(id));
        let in_team = match team {
            Some(team) => format!("owner_team = {}", st.text(team)),
            None => "owner_team IS NULL".to_owned(),
        };
        st.sql(&format!(
            "SELECT {cols} FROM mem_quarantine WHERE tenant = {p_t} AND {in_team} AND id = {p_id}"
        ));
        self.fetch(st)
            .await?
            .first()
            .map(row_to_quarantine)
            .transpose()
    }

    async fn drop_held_of_fact(
        &self,
        tenant: &str,
        team: Option<&str>,
        fact_id: &str,
    ) -> Result<u64, StoreError> {
        let mut st = self.stmt();
        let (p_t, p_f) = (st.text(tenant), st.text(fact_id));
        let in_team = match team {
            Some(team) => format!("owner_team = {}", st.text(team)),
            None => "owner_team IS NULL".to_owned(),
        };
        st.sql(&format!(
            "DELETE FROM mem_quarantine WHERE tenant = {p_t} AND {in_team} AND fact_id = {p_f}"
        ));
        self.execute(st).await
    }

    async fn expire_held(&self, tenant: &str, before: DateTime<Utc>) -> Result<u64, StoreError> {
        let mut st = self.stmt();
        let (p_t, p_b) = (st.text(tenant), st.ts(before));
        st.sql(&format!(
            "DELETE FROM mem_quarantine WHERE tenant = {p_t} AND quarantined_at < {p_b}"
        ));
        self.execute(st).await
    }
}

/// The `mem_quarantine` select list.
fn quarantine_cols(d: SqlDialect) -> String {
    format!(
        "id, tenant, owner_team, owner_user, {}, {}, source_id, memory_class, fact_class, \
         trust_score, confidence, salience, is_instruction_like, pii_review, idempotency_key, \
         reason, quarantined_at, collection, {}, {}, {}, fact_id",
        json_col(d, "content", "content"),
        json_col(d, "entities", "entities"),
        json_col(d, "entities_asserted", "entities_asserted"),
        json_col(d, "links_asserted", "links_asserted"),
        json_col(d, "derived_from", "derived_from"),
    )
}

fn row_to_quarantine(r: &SqlRow) -> Result<QuarantineRecord, StoreError> {
    let decode = |column: &str| -> Result<Json, StoreError> { r.json(column) };
    let bad = |e: serde_json::Error| StoreError::Internal(format!("decode quarantine: {e}"));
    Ok(QuarantineRecord {
        id: r.text("id")?,
        content: decode("content")?,
        entities: serde_json::from_value(decode("entities")?).map_err(bad)?,
        source_id: r.opt_text("source_id")?,
        memory_class: serde_json::from_value(Json::String(r.text("memory_class")?)).map_err(bad)?,
        fact_class: r.opt_text("fact_class")?,
        owner: owner_of(r)?,
        trust_score: r.float("trust_score")?,
        confidence: r.float("confidence")?,
        salience: r.float("salience")?,
        is_instruction_like: r.boolean("is_instruction_like")?,
        pii_review: r.boolean("pii_review")?,
        idempotency_key: r.opt_text("idempotency_key")?,
        fact_id: r.opt_text("fact_id")?,
        reason: r.text("reason")?,
        quarantined_at: r.ts("quarantined_at")?,
        collection: r.text("collection")?,
        entities_asserted: serde_json::from_value(decode("entities_asserted")?).map_err(bad)?,
        links_asserted: serde_json::from_value(decode("links_asserted")?).map_err(bad)?,
        derived_from: serde_json::from_value(decode("derived_from")?).map_err(bad)?,
    })
}

/// C7 per-tenant cycle bookkeeping in `mem_maintenance_state` (one row per tenant).
#[async_trait]
impl MaintenanceStateStore for SqlStorage {
    async fn maintenance_state(
        &self,
        tenant: &str,
    ) -> Result<Option<(Option<DateTime<Utc>>, Option<String>)>, StoreError> {
        let mut st = self.stmt();
        let p_t = st.text(tenant);
        st.sql(&format!(
            "SELECT last_cycle_at, embed_model_version FROM mem_maintenance_state \
             WHERE tenant = {p_t}"
        ));
        let rows = self.fetch(st).await?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        Ok(Some((
            row.opt_ts("last_cycle_at")?,
            row.opt_text("embed_model_version")?,
        )))
    }

    async fn set_maintenance_state(
        &self,
        tenant: &str,
        last_cycle_at: DateTime<Utc>,
        embed_model_version: &str,
    ) -> Result<(), StoreError> {
        let mut st = self.stmt();
        let (p_t, p_at, p_v) = (
            st.text(tenant),
            st.ts(last_cycle_at),
            st.text(embed_model_version),
        );
        st.sql(&format!(
            "INSERT INTO mem_maintenance_state (tenant, last_cycle_at, embed_model_version) \
             VALUES ({p_t}, {p_at}, {p_v}) \
             ON CONFLICT (tenant) DO UPDATE SET last_cycle_at = excluded.last_cycle_at, \
             embed_model_version = excluded.embed_model_version"
        ));
        self.execute(st).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_embedding_round_trips_through_its_stored_form() {
        let v = vec![0.5f32, -1.25, 3.0, 0.0];
        assert_eq!(decode_embedding(&encode_embedding(&v), 4).unwrap(), v);
    }

    #[test]
    fn a_stored_embedding_of_the_wrong_length_is_refused() {
        let err = decode_embedding(&encode_embedding(&[1.0, 2.0]), 4).unwrap_err();
        assert!(err.to_string().contains("stored embedding dimension 2"));
    }

    #[test]
    fn cosine_is_one_for_parallel_and_zero_for_orthogonal_or_empty_vectors() {
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-9);
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    /// The cached scan scores every fact with exactly the value, bit for bit, the row scan's `cosine`
    /// gave, and orders and cuts as it did.
    #[test]
    fn the_cached_scan_scores_and_orders_exactly_as_the_row_scan() {
        const D: usize = 48;
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 2001) as f32 / 1000.0 - 1.0
        };
        let query: Vec<f32> = (0..D).map(|_| next()).collect();
        let mut slab = HashMap::new();
        let mut expected = Vec::new();
        for i in 0..300 {
            // Every fifth vector repeats an earlier one, so ties are broken by id.
            let v: Vec<f32> = if i % 5 == 4 {
                (0..D).map(|j| ((i / 5 + j) % 3) as f32).collect()
            } else {
                (0..D).map(|_| next()).collect()
            };
            let id = format!("fact:{:03}", (i * 37) % 300);
            expected.push((id.clone(), cosine(&query, &v).clamp(0.0, 1.0)));
            slab.insert(
                Arc::<str>::from(id.as_str()),
                cache::CachedFact {
                    owner_user: Arc::from("u"),
                    owner_team: None,
                    visibility: Visibility::UserPrivate,
                    memory_class: MemoryClass::Semantic,
                    valid_from: DateTime::<Utc>::MIN_UTC,
                    valid_to: None,
                    ingested_at: DateTime::<Utc>::MIN_UTC,
                    entities: Arc::from([]),
                    embedding: cache::Embedding::Vector {
                        norm_sq: cache::norm_sq(&v),
                        values: Arc::from(v),
                    },
                },
            );
        }
        expected.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        expected.truncate(50);
        let filter = SearchFilter {
            user: "u".into(),
            teams: HashSet::new(),
            valid_at: None,
            memory_class: None,
            visibility: None,
            entity_facts: None,
        };
        let got = cache::vector_run(&slab, &filter, &query, cache::norm_sq(&query), D as u32, 50)
            .unwrap();
        assert_eq!(got.len(), 50);
        for (g, e) in got.iter().zip(&expected) {
            assert_eq!(g.0, e.0);
            assert_eq!(g.1.to_bits(), e.1.to_bits(), "{}", g.0);
        }
    }

    /// The cache compares validity at the precision each dialect stores and binds a timestamp.
    #[test]
    fn valid_at_is_compared_at_the_stored_precision() {
        let at = DateTime::parse_from_rfc3339("2026-03-04T05:06:07.123456789Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            at_stored_precision(SqlDialect::Sqlite, at).to_rfc3339(),
            "2026-03-04T05:06:07.123+00:00"
        );
        assert_eq!(
            at_stored_precision(SqlDialect::Postgres, at).to_rfc3339(),
            "2026-03-04T05:06:07.123456+00:00"
        );
    }

    #[test]
    fn fts5_terms_are_quoted_strings_with_inner_quotes_doubled() {
        let terms = vec!["orders".to_string(), "a\"b".to_string(), "OR".to_string()];
        assert_eq!(fts5_terms(&terms), "\"orders\" \"a\"\"b\" \"OR\"");
    }

    #[test]
    fn statement_text_carries_placeholders_never_values() {
        let ctx = ScopeContext {
            tenant: "t'; DROP TABLE mem_fact; --".into(),
            teams: vec!["x' OR '1'='1".into()],
            user: "u".into(),
            token_jti: "j".into(),
            allowed_ops: crate::types::scope::OpSet {
                read: true,
                write: true,
                forget: true,
            },
            client: None,
            fact_authority: None,
            correlation_id: "c".into(),
        };
        for d in [SqlDialect::Sqlite, SqlDialect::Postgres] {
            let mut st = Stmt::new(d);
            let f = fact_read_filter(&mut st, &ctx);
            assert!(!f.contains("DROP") && !f.contains("'1'='1"), "{f}");
        }
    }

    #[test]
    fn the_erasure_set_names_every_tenant_carrying_table_of_the_ddl() {
        for (dialect, sql) in [
            (
                SqlDialect::Sqlite,
                [ddl::SQLITE_INIT, ddl::SQLITE_GRAPH, ddl::SQLITE_JOB_FACT].concat(),
            ),
            (
                SqlDialect::Postgres,
                [
                    ddl::POSTGRES_INIT,
                    ddl::POSTGRES_GRAPH,
                    ddl::POSTGRES_JOB_FACT,
                ]
                .concat(),
            ),
        ] {
            let tables: Vec<&str> = sql
                .split("CREATE TABLE ")
                .skip(1)
                .filter_map(|chunk| {
                    let name = chunk.split([' ', '(']).next()?;
                    let body = chunk.split(");").next()?;
                    body.contains("tenant TEXT").then_some(name)
                })
                .collect();
            for t in &tables {
                assert!(ERASURE_SET.contains(t), "{dialect:?}: {t} missing");
            }
            assert_eq!(tables.len(), ERASURE_SET.len(), "{dialect:?}");
        }
    }

    #[test]
    fn migrations_are_one_step_per_dialect() {
        assert_eq!(
            SqlStorage::migrations(SqlDialect::Sqlite)[0].0,
            ddl::STEP_INIT
        );
        assert_eq!(
            SqlStorage::migrations(SqlDialect::Postgres)[0].0,
            ddl::STEP_INIT
        );
    }
}
