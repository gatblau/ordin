//! C1 — Memory Store. The persistence layer and single owner of the embedded SurrealDB connection.
//! Implements the `MemoryStore` trait over SurrealDB 3.x with one namespace per tenant (ADR-011), the
//! three retrieval signals (HNSW vector, BM25 keyword, 2-hop graph) surfaced through one `recall`
//! operation, bi-temporal writes (ADR-002), and verifiable `hard_delete` (SA-DELETE-01).
//!
//! All caller input (content, vectors, names, ids) is bound as query parameters — never
//! string-interpolated — per the sql-safety layer rule. Tenant identifiers (which select the
//! namespace and cannot be parameter-bound in DDL) are validated by `migrate::validate_tenant`.

pub(crate) mod convert;
pub mod migrate;

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use surrealdb::engine::any::Any;
use surrealdb::types::Value;
use surrealdb::Surreal;

use crate::config::Config;
use crate::types::api::{DeletionProof, TenantErasureProof};
use crate::types::domain::{Entity, Fact, Relationship, Source};
use crate::types::placement::{self, MemoryProfile, Placement};
use crate::types::ports::{AuditEntry, Candidate, MemoryStore, StageOneQuery, StoreError};
use crate::types::scope::{can_read, ScopeContext, ScopeRef};

use migrate::{map_db_err, validate_tenant, Migrator, DB_NAME};

/// The SurrealDB memory store. The connection is typed over `surrealdb::engine::any::Any`, so the
/// same `Store` carries either an embedded engine (SurrealKV / in-memory) or a remote endpoint
/// (`ws(s)://` / `http(s)://`) — the engine is resolved from the endpoint scheme at connect time.
pub struct Store {
    db: Surreal<Any>,
    embed_dim: u32,
    /// Tenants whose schema has been provisioned in this process. Purely a cost cache — see
    /// `ensure_provisioned`.
    provisioned: std::sync::RwLock<std::collections::HashSet<String>>,
    /// Which deployment scenario this store serves. Selects how a logical tenant maps to a physical
    /// namespace (`types::placement::resolve`) and which retrieval strategy stage-1 uses. Placement
    /// still resolves to identity under both profiles; retrieval does not.
    profile: MemoryProfile,
}

impl Store {
    /// Open the store from the loaded configuration. `MEMORY_STORE_REMOTE_URL` (if set) wins over the
    /// embedded path with a startup warning (Phase 2D precedence); otherwise the embedded SurrealKV
    /// engine is opened at `MEMORY_STORE_PATH`. The connection is built through
    /// `surrealdb::engine::any::connect`, which resolves the engine from the endpoint scheme
    /// (`surrealkv://` embedded, `ws(s)://`/`http(s)://` remote — ADR-009 scale-out), so a remote
    /// deployment needs no code change. On any connection failure returns `StoreError::Unavailable`.
    pub async fn connect(cfg: &Config) -> Result<Self, StoreError> {
        let endpoint = if let Some(remote) = &cfg.store_remote_url {
            if !cfg.store_path.is_empty() {
                tracing::warn!(
                    target: "memory",
                    "store.connect: both MEMORY_STORE_REMOTE_URL and MEMORY_STORE_PATH set; remote wins"
                );
            }
            tracing::info!(target: "memory", backend = "remote", "store.connect");
            remote.clone()
        } else {
            tracing::info!(target: "memory", backend = "surrealkv", "store.connect");
            format!("surrealkv://{}", cfg.store_path)
        };
        let db = surrealdb::engine::any::connect(endpoint)
            .await
            .map_err(map_db_err)?;
        // Authenticate to a secured remote server (FU-019). Config guarantees both-or-neither, so a
        // present user implies a present password. Embedded and no-auth remote endpoints skip signin;
        // Namespace selection is per statement, not per session: every statement carries its own
        // `USE NS <t> DB memory;` via `Migrator::scoped` (RISK-012), so a root signin here is sufficient
        // and no connection session state is mutated afterwards.
        if cfg.store_remote_url.is_some() {
            if let (Some(user), Some(pass)) = (&cfg.store_remote_user, &cfg.store_remote_pass) {
                db.signin(surrealdb::opt::auth::Root {
                    username: user.clone(),
                    password: pass.expose().to_string(),
                })
                .await
                .map_err(map_db_err)?;
            }
        }
        Ok(Self {
            db,
            embed_dim: cfg.embed_dim,
            provisioned: Default::default(),
            profile: cfg.memory_profile,
        })
    }

    /// Open an in-memory store (the real engine, in-process) for tests and ephemeral runs, under the
    /// default (`collective`) profile.
    pub async fn new_in_memory(embed_dim: u32) -> Result<Self, StoreError> {
        Self::new_in_memory_with_profile(embed_dim, MemoryProfile::default()).await
    }

    /// As `new_in_memory`, under an explicit profile. Exists so the two retrieval strategies can be
    /// exercised against the same real engine and the same data — a strategy that is only ever tested
    /// on its own data has not been shown to be equivalent to the one it must not regress.
    pub async fn new_in_memory_with_profile(
        embed_dim: u32,
        profile: MemoryProfile,
    ) -> Result<Self, StoreError> {
        Self::open("mem://", embed_dim, profile).await
    }

    /// Open a store at an arbitrary endpoint under an explicit profile, taking no `Config`.
    ///
    /// `connect` is the production path and derives everything from configuration; this is the path
    /// for a caller that already knows exactly which store it wants and must not inherit anything —
    /// the benchmark harness, whose whole first safety gate is that it never picks up a live
    /// deployment's store configuration (SA-BENCH-ISOLATION-01). It exists because the alternatives
    /// were both wrong: `new_in_memory_with_profile` cannot reach `surrealkv://`, and an absolute
    /// latency claim measured on `mem://` is not a claim about the deployed engine.
    ///
    /// **Performs no signin.** It takes no credentials, so a secured remote endpoint belongs on
    /// `connect`, which reads the credential pair from configuration.
    pub async fn open(
        endpoint: &str,
        embed_dim: u32,
        profile: MemoryProfile,
    ) -> Result<Self, StoreError> {
        let db = surrealdb::engine::any::connect(endpoint.to_string())
            .await
            .map_err(map_db_err)?;
        Ok(Self {
            db,
            embed_dim,
            provisioned: Default::default(),
            profile,
        })
    }

    /// A `Migrator` bound to this store's connection.
    fn migrator(&self) -> Migrator<'_> {
        Migrator::new(&self.db, self.embed_dim)
    }

    /// Share the SurrealDB connection (the engine handle is internally `Arc`-backed, so a clone reuses
    /// the same database — the same in-process engine when embedded, or the same network connection
    /// when remote). The Durable Work Queue (C2) is built over this handle so its
    /// `work_job`/`dead_letter` tables live inside the same engine as the C1 store (SA-QUEUE-01:
    /// store-backed default, single-binary deployment intact).
    pub fn handle(&self) -> Surreal<Any> {
        self.db.clone()
    }

    /// Where `tenant`'s data physically lives under the active profile.
    fn placement(&self, tenant: &str) -> Placement {
        placement::resolve(self.profile, tenant, DB_NAME)
    }

    /// Bind `sql` to `tenant`'s physical namespace, in the same statement batch (RISK-012).
    ///
    /// This is the **single point** at which a logical tenant becomes a physical namespace. Every
    /// statement C1 issues for a tenant goes through here, so changing the mapping (Stage 2, many
    /// tenants per namespace) is a change to `placement::resolve` alone rather than to the 38 call
    /// sites that select a namespace.
    fn scoped(&self, tenant: &str, sql: impl AsRef<str>) -> Result<String, StoreError> {
        let p = self.placement(tenant);
        Migrator::scoped_at(&p.namespace, &p.database, sql)
    }

    /// Ensure `tenant`'s schema exists, so a never-seen tenant is provisioned lazily and idempotently.
    ///
    /// This **selects nothing**. Namespace selection travels with each statement via
    /// `Migrator::scoped` (RISK-012): mutating a shared connection session here and running the
    /// statement as a separate await let a concurrent task switch the namespace in between, which
    /// misplaced 70-75% of writes under concurrent multi-tenant load.
    ///
    /// Provisioning is cached per namespace for the process lifetime. `migrate_up` is idempotent, so a
    /// concurrent double-provision is harmless; the cache exists to keep the four-round-trip migration
    /// preamble off every operation, not for correctness. A namespace is inserted only after
    /// `migrate_up` succeeds, so a failed provision is retried on the next call.
    async fn ensure_provisioned(&self, tenant: &str) -> Result<(), StoreError> {
        validate_tenant(tenant)?;
        if self
            .provisioned
            .read()
            .map_err(|_| StoreError::Internal("provisioned cache poisoned".into()))?
            .contains(&self.placement(tenant).namespace)
        {
            return Ok(());
        }
        // Provision the *namespace*, and cache on it: under a profile where several tenants share a
        // namespace (Stage 2), provisioning is per namespace and must not be repeated per tenant.
        let ns = self.placement(tenant).namespace;
        self.migrator().migrate_up(&ns).await?;
        self.provisioned
            .write()
            .map_err(|_| StoreError::Internal("provisioned cache poisoned".into()))?
            .insert(ns);
        Ok(())
    }
}

/// Build the SurrealDB record-id literal for a "table:key" domain id, e.g. `fact:⟨uuid⟩`, as a
/// parameter-bindable record-id value. The id is split on the first ':' so an arbitrary uuid key
/// (which may contain '-') is bound as the record key, never interpolated.
fn id_thing(id: &str) -> Result<Value, StoreError> {
    let (table, key) = id
        .split_once(':')
        .ok_or_else(|| StoreError::Validation(format!("malformed id: {id}")))?;
    Ok(Value::RecordId(surrealdb::types::RecordId::new(
        table.to_string(),
        key.to_string(),
    )))
}

/// Validate a fact against the binding domain rules (Shared Context). Returns `StoreError::Validation`
/// with a message marking score-range failures with the word "range" so the X1 mapping selects
/// `VAL_OUT_OF_RANGE` (else `VAL_INVALID_BODY`).
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
    Ok(())
}

/// The read-filter `WHERE` fragment plus its bound parameters, built from `ctx`. Applied to every
/// fact read so a caller never sees a record outside its scope (§2C.3). Visibility-bearing tables
/// (fact) use the full rule; entity/relationship/source use the reduced rule (no visibility).
fn fact_filter_clause() -> &'static str {
    "owner.tenant = $ctenant AND (owner.user = $cuser \
     OR (visibility = 'team-shared' AND owner.team IN $cteams) \
     OR visibility = 'tenant-shared' \
     OR id IN (SELECT VALUE type::record(fact) FROM grant \
               WHERE grantee = $cuser AND revoked_at IS NONE \
                 AND valid_from <= $cnow AND valid_to > $cnow))"
}

/// The consent-grant arm on its own, for the equivalence test against
/// `Grant::is_active_at`. The two express one rule and must not drift.
///
/// `type::record(fact)` is load-bearing rather than decorative: `grant.fact` is a string and
/// `fact.id` is a record link, so the bare comparison silently matches nothing and every grantee
/// is denied while every test over the fragment text still passes (FU-026).
#[cfg(test)]
pub(crate) fn grant_arm_sql() -> &'static str {
    "grantee = $cuser AND revoked_at IS NONE AND valid_from <= $cnow AND valid_to > $cnow"
}

/// Whether `ctx` is the C7 maintenance/admin scope: the `"maintenance"` jti marker with an empty user
/// (built by C7 from a tenant id alone, never from request input). A maintenance scope operates on the
/// WHOLE tenant namespace — the per-user/visibility read filter is bypassed so decay, supersession, and
/// verifiable hard-delete reach user-private and team-shared facts, not only tenant-shared ones
/// (RISK-009). Tenant isolation stays structural: every store method first selects the tenant namespace
/// (`ensure_and_use`), so a maintenance scope can never cross tenants.
fn is_maintenance_scope(ctx: &ScopeContext) -> bool {
    ctx.token_jti == "maintenance" && ctx.user.is_empty()
}

/// The fact read-filter `WHERE` fragment for `ctx`: the whole tenant for a maintenance scope, else the
/// per-user/visibility filter. The `$cuser`/`$cteams` bindings are still supplied by callers; when this
/// returns `true` they are simply unreferenced.
fn fact_read_filter(ctx: &ScopeContext) -> &'static str {
    if is_maintenance_scope(ctx) {
        // Not `true`. A maintenance scope bypasses the per-user and per-visibility arms so decay,
        // supersession and verifiable hard-delete reach user-private and team-shared facts — but it
        // must not bypass the tenant. `true` was safe only while a namespace held exactly one tenant
        // (RISK-017); it would let a sweep mutate a co-resident tenant's facts under RFC-03 Stage 2.
        "owner.tenant = $ctenant"
    } else {
        fact_filter_clause()
    }
}

/// How stage-1 recovers candidates from the vector signal.
///
/// The two deployment scenarios want opposite things here, and the deciding property is the
/// **expected visible fraction per caller** — how much of a namespace's facts the scope read-filter
/// admits (see `types::placement`):
///
/// * **High visible fraction** (collective memory — team-shared and tenant-shared facts): the ANN
///   index returns candidates that mostly survive filtering, so it does real work.
/// * **Low visible fraction** (individual memory — a caller sees only their own facts): the index
///   returns candidates that almost all filter out, so the result set starves. Over-fetch is a fixed
///   4x multiple of `stage1_k`, so a caller must see roughly 25% of a namespace's facts for it to
///   hold — and no configuration moves that bound.
///
/// Neither strategy is universally better — they trade an index walk against a filtered scan, and
/// which wins is decided by the visible fraction, not by preference.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum VectorStrategy {
    /// Walk the `fact_hnsw` index for K approximate nearest neighbours, then AND the scope, validity
    /// and metadata predicates on top. Correct wherever the visible fraction is high.
    PostFilteredAnn,
    /// Apply the scope, validity and metadata predicates first — driven by `fact_owner_user` /
    /// `fact_owner_team` — then score every surviving row exactly with `vector::similarity::cosine`.
    ///
    /// This cannot starve: filtering happens before the candidate set is capped, so if a visible fact
    /// exists it is scored. The cost is O(V) in the *visible* set rather than O(log N) in the whole
    /// namespace, which is why it is not the collective default — it is the right trade only where V
    /// is small, which is exactly the condition that makes the ANN index useless.
    FilterFirstExact,
}

impl VectorStrategy {
    /// The strategy a profile implies.
    ///
    /// The branch is on the visible-fraction expectation rather than on the profile variant, because
    /// that property — not the profile's name — is what decides which strategy is correct.
    pub(crate) fn for_profile(profile: MemoryProfile) -> Self {
        if profile.expects_high_visible_fraction() {
            Self::PostFilteredAnn
        } else {
            Self::FilterFirstExact
        }
    }
}

/// The scope-only read filter for the **vector** path — the three scope arms without the consent-grant
/// arm (SA-GRANT-08).
///
/// The omission is deliberate and is the load-bearing decision of the grant model. Both vector
/// strategies are wrong for a grant-scoped read, for different reasons. `PostFilteredAnn` walks the
/// HNSW index for K neighbours and filters afterwards, so at the low visible fraction a grant produces
/// — a handful of facts among thousands — a granted fact is crowded out of the candidate window by
/// facts the caller cannot read, and the search returns nothing while looking as though it worked.
/// `FilterFirstExact` filters first, but its `WITH INDEX fact_owner_user` hint is sound only because
/// `read` is a narrow owner equality; a grant arm is a sub-select, and forcing a single-column index
/// under it would drop the arm entirely.
///
/// So grant-scoped semantic search is not served rather than served incompletely. A caller whose only
/// access to a fact is a grant reaches it by direct fetch, keyword search or entity traversal — none of
/// which applies an approximate candidate window, and all of which use `fact_filter_clause`.
fn vector_scope_filter_clause() -> &'static str {
    "owner.tenant = $ctenant AND (owner.user = $cuser \
     OR (visibility = 'team-shared' AND owner.team IN $cteams) \
     OR visibility = 'tenant-shared')"
}

/// The scope read-filter the stage-1 signals apply, and the index that can drive it, under `profile`.
///
/// The two profiles disagree about what a caller can read, and that disagreement is *definitional*
/// rather than an optimisation: the individual profile means every user has their own memory and shares
/// nothing, so `team-shared` and `tenant-shared` facts do not exist. Stating that as a narrower
/// predicate is what gives the query an index to stand on — `fact_owner_user` is a plain equality
/// index, and the collective filter's three-way `OR` cannot use it.
///
/// **The narrow filter is a strict subset of the broad one**, so selecting the individual profile can
/// only ever return *fewer* rows, never rows a caller could not already read. The failure mode if a
/// deployment mislabels itself is under-retrieval of its shared facts, not disclosure — which is the
/// direction a scope filter should fail in.
fn vector_read_filter(profile: MemoryProfile) -> (&'static str, Option<&'static str>) {
    if profile.expects_high_visible_fraction() {
        (vector_scope_filter_clause(), None)
    } else {
        (
            "owner.tenant = $ctenant AND owner.user = $cuser",
            Some("fact_owner_user"),
        )
    }
}

/// Build the stage-1 vector-signal statement for `strategy`.
fn vector_signal_sql(
    strategy: VectorStrategy,
    k: i64,
    read: &str,
    hint: Option<&str>,
    validity: &str,
    meta: &str,
) -> String {
    match strategy {
        VectorStrategy::PostFilteredAnn => {
            // The `<|K,EF|>` operator drives the `fact_hnsw` index: it returns the K approximate
            // nearest neighbours (EF = search breadth) and `vector::distance::knn()` reports each
            // match's cosine distance, computed during the index walk. This is an O(log N) index
            // lookup, not the prior O(N) `vector::similarity::cosine(...) ORDER BY` brute-force scan.
            // The scope read-filter, validity, and metadata predicates are AND-ed on top
            // (KNN-then-filter); K is over-fetched (4x stage1_k) so filtering rarely starves the final
            // stage-1 set where the visible fraction is high, then the result is ordered by ascending
            // distance and capped to stage1_k. Cosine similarity is recovered as `1 - distance`,
            // clamped to [0,1] by the caller.
            let knn_k = (k as usize).saturating_mul(4).clamp(k as usize, 1024);
            let ef = knn_k.max(64);
            format!(
                "SELECT meta::id(id) AS fid, vector::distance::knn() AS dist \
                 FROM fact WHERE embedding <|{knn_k},{ef}|> $qv AND ({read}) AND {validity}{meta} \
                 ORDER BY dist ASC LIMIT $k"
            )
        }
        VectorStrategy::FilterFirstExact => {
            // No index walk: the scope read-filter, validity and metadata predicates select the
            // visible set first (served by `fact_owner_user` / `fact_owner_team`), and every surviving
            // row is then scored exactly. Because the cap is applied after filtering rather than
            // before it, a visible fact can never be crowded out of the candidate set by facts the
            // caller cannot read — the starvation mode that makes `PostFilteredAnn` wrong at a low
            // visible fraction.
            //
            // `embedding IS NOT NONE` is required, not defensive: the field is `option<array<float>>`
            // and a fact is created before `set_fact_embedding` runs, so unembedded rows are a normal
            // steady state, and scoring one has no meaning.
            //
            // The projection is a distance (`1 - similarity`) rather than a similarity so that both
            // strategies return the same row shape — `(fid, dist)`, ascending — and the caller needs
            // no knowledge of which one ran. Cosine similarity is in [-1,1], so this distance is in
            // [0,2]; the caller's `(1 - dist).clamp(0,1)` recovers the same [0,1] score as the index
            // path, clamping an anti-correlated vector to 0 exactly as the ANN arm does.
            // The index hint is not a micro-optimisation — without it this strategy is *slower* than
            // the one it replaces. SurrealDB 3.1.5's planner has no selectivity statistics, and given
            // `owner.user = $cuser AND valid_to IS NONE` it picks `fact_valid`, an index on `valid_to`
            // that matches every live fact, over `fact_owner_user`, which matches one user's. Measured
            // at 2,000 facts / 1,024 dimensions: 28.0 ms planner's choice, 0.26 ms hinted.
            //
            // The hint is only sound because `read` is the narrow owner equality (`vector_read_filter`)
            // — forcing a single-column index under the collective profile's three-way `OR` would drop
            // the shared-fact arms. The two are chosen together for that reason.
            let _ = k; // the cap travels as `$k`; no operator-syntax integer is needed here.
            let with_index = match hint {
                Some(idx) => format!(" WITH INDEX {idx}"),
                None => String::new(),
            };
            format!(
                "SELECT meta::id(id) AS fid, 1 - vector::similarity::cosine(embedding, $qv) AS dist \
                 FROM fact{with_index} WHERE ({read}) AND {validity}{meta} AND embedding IS NOT NONE \
                 ORDER BY dist ASC LIMIT $k"
            )
        }
    }
}

/// The optional metadata `WHERE` fragments (memory_class / visibility / entity) shared by every
/// stage-1 signal.
///
/// Extracted so the vector leg and `recall` build it from one definition. They must agree exactly: a
/// benchmark that timed a query with a different predicate set from the one production runs would be
/// measuring a query nobody executes.
fn meta_clause(q: &StageOneQuery) -> String {
    let mut meta = String::new();
    if q.filters.memory_class.is_some() {
        meta.push_str(" AND memory_class = $mclass");
    }
    if q.filters.visibility.is_some() {
        meta.push_str(" AND visibility = $vis");
    }
    if q.filters.entity.is_some() {
        meta.push_str(" AND $entity IN entities");
    }
    meta
}

/// The reduced read-filter `WHERE` fragment for tables with no `visibility` field.
fn novis_filter_clause() -> &'static str {
    "owner.tenant = $ctenant AND (owner.user = $cuser OR owner.team IN $cteams)"
}

/// The bi-temporal validity `WHERE` fragment for a `valid_at` filter (or currently-valid when unset).
fn validity_clause(valid_at: Option<DateTime<Utc>>) -> &'static str {
    if valid_at.is_some() {
        "valid_from <= $valid_at AND (valid_to IS NONE OR valid_to > $valid_at)"
    } else {
        "valid_to IS NONE"
    }
}

/// Test-only helpers for the consent-grant read arm. Inherent rather than part of the
/// `MemoryStore` trait: planting a grant row is not a store capability the service uses.
#[cfg(test)]
impl Store {
    /// Plant a grant row directly, for behavioural tests of the read filter's fourth arm.
    ///
    /// Test-only until the Consent Grants component's `issue` lands. It exists now because the arm
    /// shipped inert for want of exactly this: a test that puts a row in the table and asks whether
    /// the grantee can read (FU-026).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn put_grant_for_test(
        &self,
        tenant: &str,
        grant_id: &str,
        fact_id: &str,
        grantor: &ScopeRef,
        grantee: &str,
        valid_from: DateTime<Utc>,
        valid_to: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.ensure_provisioned(tenant).await?;
        self.db
            .query(self.scoped(
                tenant,
                "UPSERT $id CONTENT { fact: $fact, grantor: $grantor, grantee: $grantee, \
                 purpose: $purpose, operation: 'read', valid_from: $vf, valid_to: $vt, \
                 revoked_at: NONE, evidence: $evidence, created_at: $now }",
            )?)
            .bind(("id", id_thing(grant_id)?))
            .bind(("fact", fact_id.to_string()))
            .bind(("grantor", {
                // `team` is `option<string>`; serde renders `None` as JSON null, which
                // SurrealDB refuses to coerce to NONE. Omit the key instead.
                let mut o = serde_json::Map::new();
                o.insert("tenant".into(), grantor.tenant.clone().into());
                o.insert("user".into(), grantor.user.clone().into());
                if let Some(team) = &grantor.team {
                    o.insert("team".into(), team.clone().into());
                }
                serde_json::Value::Object(o)
            }))
            .bind(("grantee", grantee.to_string()))
            .bind(("purpose", "behavioural test".to_string()))
            .bind(("vf", valid_from))
            .bind(("vt", valid_to))
            .bind(("evidence", "test".to_string()))
            .bind(("now", Utc::now()))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    /// Revoke a planted grant, for the revocation-is-immediate test.
    pub(crate) async fn revoke_grant_for_test(
        &self,
        tenant: &str,
        grant_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.db
            .query(self.scoped(tenant, "UPDATE $id SET revoked_at = $at")?)
            .bind(("id", id_thing(grant_id)?))
            .bind(("at", at))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }
}

#[async_trait]
impl MemoryStore for Store {
    async fn put_fact(&self, f: &Fact) -> Result<(), StoreError> {
        validate_fact(f)?;
        self.ensure_provisioned(&f.owner.tenant).await?;
        let thing = id_thing(&f.id)?;
        let obj = convert::fact_to_object(f);
        self.db
            .query(self.scoped(&f.owner.tenant, "UPSERT $id CONTENT $rec")?)
            .bind(("id", thing))
            .bind(("rec", Value::Object(obj)))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        tracing::debug!(target: "memory", table = "fact", correlation_id = %"", "store.put");
        Ok(())
    }

    async fn get_fact(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Fact>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let thing = id_thing(id)?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                format!("SELECT * FROM $id WHERE {}", fact_read_filter(ctx)),
            )?)
            .bind(("id", thing))
            .bind(("ctenant", ctx.tenant.clone()))
            .bind(("cuser", ctx.user.clone()))
            .bind(("cteams", ctx.teams.clone()))
            .bind(("cnow", Utc::now()))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("read fact: {e}")))?;
        match rows.into_iter().next() {
            Some(row) => Ok(Some(convert::row_to_fact(row)?)),
            None => Ok(None),
        }
    }

    async fn recall(
        &self,
        ctx: &ScopeContext,
        q: &StageOneQuery,
    ) -> Result<Vec<Candidate>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let validity = validity_clause(q.filters.valid_at);
        let k = q.stage1_k.max(1) as i64;

        // Optional metadata filters (memory_class / visibility / entity) shared by every signal.
        let meta = meta_clause(q);

        // The read-filter is a profile-level statement about what sharing exists, so all three stage-1
        // signals use the same one — a fact the keyword leg could return but the vector leg could not
        // would make relevance depend on which signal happened to match.
        // The keyword and graph legs share the vector leg's read filter, but not its index hint —
        // the hint is specific to the filter-first vector strategy and `vector_leg` applies its own.
        let (read, _) = vector_read_filter(self.profile);
        // Accumulate the per-signal scores keyed by fact id.
        let mut scores: BTreeMap<String, (f64, f64, f64)> = BTreeMap::new();

        // --- Vector signal (HNSW ANN via the KNN operator, RISK-008 / NFR-P3) ---
        // The `<|K,EF|>` operator drives the `fact_hnsw` index: it returns the K approximate nearest
        // neighbours (EF = search breadth) and `vector::distance::knn()` reports each match's cosine
        // distance, computed during the index walk. This is an O(log N) index lookup, not the prior
        // O(N) `vector::similarity::cosine(...) ORDER BY` brute-force scan. K and EF are operator
        // syntax (not bindable), so they are formatted as validated integers — never caller input.
        // The scope read-filter, validity, and metadata predicates are AND-ed on top (KNN-then-filter);
        // K is over-fetched (4x stage1_k) so filtering rarely starves the final stage-1 set, then the
        // result is ordered by ascending distance and capped to stage1_k. Cosine similarity is
        // recovered as `1 - distance`, clamped to [0,1] to match the prior signal's range.
        if !q.query_vector.is_empty() {
            let hits = self
                .vector_leg(ctx, q, VectorStrategy::for_profile(self.profile))
                .await?;
            for (fid, similarity) in hits {
                scores.entry(fid).or_default().0 = similarity;
            }
        }

        // --- Keyword signal (BM25) ---
        if !q.keyword_terms.is_empty() {
            let terms = q.keyword_terms.join(" ");
            let sql = format!(
                "SELECT meta::id(id) AS fid, search::score(0) AS s FROM fact \
                 WHERE content_text @0@ $terms AND ({read}) AND {validity}{meta} \
                 ORDER BY s DESC LIMIT $k"
            );
            let mut resp = self
                .bind_recall(self.db.query(self.scoped(&ctx.tenant, &sql)?), ctx, q)
                .bind(("terms", terms))
                .bind(("k", k))
                .await
                .map_err(map_db_err)?;
            let rows: Vec<Json> = resp
                .take(1)
                .map_err(|e| StoreError::Internal(format!("keyword signal: {e}")))?;
            let max = rows
                .iter()
                .filter_map(|r| r.get("s").and_then(|v| v.as_f64()))
                .fold(0.0_f64, f64::max);
            for r in rows {
                if let (Some(fid), Some(s)) = (
                    r.get("fid").and_then(|v| v.as_str()),
                    r.get("s").and_then(|v| v.as_f64()),
                ) {
                    let norm = if max > 0.0 {
                        (s / max).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    scores.entry(fid.to_string()).or_default().1 = norm;
                }
            }
        }

        // --- Graph signal (2-hop traversal from the filter entity) ---
        if let Some(entity) = &q.filters.entity {
            // `WITH INDEX fact_ent` is required, not an optimisation (RISK-018). Given
            // `$entity IN entities AND … AND valid_to IS NONE`, SurrealDB 3.1.5's planner picks
            // `fact_valid` — an index on `valid_to` that matches every live fact — over `fact_ent`,
            // which matches only the facts mentioning this entity. The planner has no selectivity
            // statistics, and adding the bi-temporal predicate is what flips the choice.
            //
            // The hint depends on migration 0002: under 0001's `fact_ent ON fact FIELDS entities` the
            // index covers the array as a whole value, cannot serve element matching, and hinting it
            // produced a full table scan — worse than the planner's own choice.
            //
            // The hint is sound here because `$entity IN entities` is a *conjunct*: the index supplies
            // a superset and the scope and validity predicates are applied by a filter above it. That
            // is why the same hint would be wrong on the collective read-filter, which is a
            // disjunction — forcing one index there would drop the shared-fact arms rather than
            // narrow the scan.
            let sql = format!(
                "SELECT meta::id(id) AS fid FROM fact WITH INDEX fact_ent \
                 WHERE $entity IN entities AND ({read}) AND {validity} LIMIT $k"
            );
            let mut resp = self
                .db
                .query(self.scoped(&ctx.tenant, sql)?)
                .bind(("entity", entity.clone()))
                .bind(("ctenant", ctx.tenant.clone()))
                .bind(("cuser", ctx.user.clone()))
                .bind(("cteams", ctx.teams.clone()))
                .bind(("cnow", Utc::now()))
                .bind(("k", k));
            resp = bind_valid_at(resp, q.filters.valid_at);
            let mut resp = resp.await.map_err(map_db_err)?;
            let rows: Vec<Json> = resp
                .take(1)
                .map_err(|e| StoreError::Internal(format!("graph signal: {e}")))?;
            for r in rows {
                if let Some(fid) = r.get("fid").and_then(|v| v.as_str()) {
                    scores.entry(fid.to_string()).or_default().2 = 1.0;
                }
            }
        }

        // Resolve each surviving id to its full fact, re-applying the read filter, and assemble.
        let mut out = Vec::new();
        for (fid, (sem, kw, graph)) in scores.into_iter().take(q.stage1_k as usize) {
            let full = format!("fact:{fid}");
            if let Some(fact) = self.get_fact(ctx, &full).await? {
                out.push(Candidate {
                    fact_id: fact.id.clone(),
                    fact,
                    semantic_score: sem,
                    keyword_score: kw,
                    graph_score: graph,
                });
            }
        }
        tracing::debug!(target: "memory", candidate_count = out.len(), "store.recall");
        Ok(out)
    }

    async fn end_validity(
        &self,
        ctx: &ScopeContext,
        id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        // Confirm read access (else NotFound); the get is scope-checked.
        if self.get_fact(ctx, id).await?.is_none() {
            return Err(StoreError::NotFound);
        }
        let thing = id_thing(id)?;
        self.db
            .query(self.scoped(
                &ctx.tenant,
                "UPDATE $id SET valid_to = $at WHERE valid_to IS NONE",
            )?)
            .bind(("id", thing))
            .bind(("at", surrealdb::types::Datetime::from(at)))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn supersede(
        &self,
        ctx: &ScopeContext,
        old_id: &str,
        new_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        if self.get_fact(ctx, old_id).await?.is_none()
            || self.get_fact(ctx, new_id).await?.is_none()
        {
            return Err(StoreError::NotFound);
        }
        let old_thing = id_thing(old_id)?;
        let new_thing = id_thing(new_id)?;
        self.db
            .query(self.scoped(
                &ctx.tenant,
                "BEGIN; \
                 UPDATE $old SET valid_to = $at, superseded_by = $newid; \
                 UPDATE $new SET supersedes = $oldid; \
                 COMMIT;",
            )?)
            .bind(("old", old_thing))
            .bind(("new", new_thing))
            .bind(("at", surrealdb::types::Datetime::from(at)))
            .bind(("newid", new_id.to_string()))
            .bind(("oldid", old_id.to_string()))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn hard_delete(&self, ctx: &ScopeContext, id: &str) -> Result<DeletionProof, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let base = match self.get_fact(ctx, id).await? {
            Some(f) => f,
            None => return Err(StoreError::NotFound),
        };

        // Collect derived-insight facts whose derived_from contains this id (scope-checked).
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                format!(
                    "SELECT * FROM fact WHERE $base IN derived_from AND ({})",
                    fact_read_filter(ctx)
                ),
            )?)
            .bind(("base", id.to_string()))
            .bind(("ctenant", ctx.tenant.clone()))
            .bind(("cuser", ctx.user.clone()))
            .bind(("cteams", ctx.teams.clone()))
            .bind(("cnow", Utc::now()))
            .await
            .map_err(map_db_err)?;
        let derived_rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("collect derived: {e}")))?;
        let derived: Vec<Fact> = derived_rows
            .into_iter()
            .map(convert::row_to_fact)
            .collect::<Result<_, _>>()?;

        // Build the full removal set + count embeddings removed (base + each derived with an embedding).
        let mut removed_ids: Vec<String> = Vec::with_capacity(derived.len() + 1);
        removed_ids.push(base.id.clone());
        let mut embeddings_removed: u32 = 0;
        if self.fact_has_embedding(&ctx.tenant, &base.id).await? {
            embeddings_removed += 1;
        }
        for d in &derived {
            removed_ids.push(d.id.clone());
            if self.fact_has_embedding(&ctx.tenant, &d.id).await? {
                embeddings_removed += 1;
            }
        }

        // Delete every collected record in one transaction.
        let things: Vec<Value> = removed_ids
            .iter()
            .map(|i| id_thing(i))
            .collect::<Result<_, _>>()?;
        self.db
            .query(self.scoped(&ctx.tenant, "DELETE $ids")?)
            .bind(("ids", things))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;

        // Verify every collected id is gone; otherwise the delete is partial — never report complete.
        let mut still = 0u32;
        for i in &removed_ids {
            if self.get_fact(ctx, i).await?.is_some() {
                still += 1;
            }
        }
        if still > 0 {
            let expected = removed_ids.len() as u32;
            return Err(StoreError::PartialDelete {
                removed: expected - still,
                expected,
            });
        }

        // Digest over the sorted removed ids (SA-DELETE-01).
        let mut sorted = removed_ids.clone();
        sorted.sort();
        let digest = sha256_hex(&sorted);
        let derived_removed: Vec<String> = derived.iter().map(|d| d.id.clone()).collect();
        tracing::debug!(
            target: "memory",
            derived_removed_count = derived_removed.len(),
            embeddings_removed,
            "store.hard_delete"
        );
        Ok(DeletionProof {
            deleted_at: Utc::now(),
            record_id: id.to_string(),
            derived_removed,
            embeddings_removed,
            digest,
        })
    }

    async fn put_entity(&self, e: &Entity) -> Result<(), StoreError> {
        if e.canonical_name.is_empty() || e.canonical_name.chars().count() > 512 {
            return Err(StoreError::Validation("canonical_name length".into()));
        }
        self.ensure_provisioned(&e.owner.tenant).await?;
        let thing = id_thing(&e.id)?;
        self.db
            .query(self.scoped(&e.owner.tenant, "UPSERT $id CONTENT $rec")?)
            .bind(("id", thing))
            .bind(("rec", Value::Object(convert::entity_to_object(e))))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn get_entity(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Entity>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let thing = id_thing(id)?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                format!("SELECT * FROM $id WHERE {}", novis_filter_clause()),
            )?)
            .bind(("id", thing))
            .bind(("ctenant", ctx.tenant.clone()))
            .bind(("cuser", ctx.user.clone()))
            .bind(("cteams", ctx.teams.clone()))
            .bind(("cnow", Utc::now()))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("read entity: {e}")))?;
        match rows.into_iter().next() {
            Some(row) => Ok(Some(convert::row_to_entity(row)?)),
            None => Ok(None),
        }
    }

    async fn find_entity_by_name(
        &self,
        ctx: &ScopeContext,
        name: &str,
    ) -> Result<Vec<Entity>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                format!(
                "SELECT * FROM entity WHERE (canonical_name = $name OR $name IN aliases) AND ({})",
                novis_filter_clause()
            ),
            )?)
            .bind(("name", name.to_string()))
            .bind(("ctenant", ctx.tenant.clone()))
            .bind(("cuser", ctx.user.clone()))
            .bind(("cteams", ctx.teams.clone()))
            .bind(("cnow", Utc::now()))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("find entity: {e}")))?;
        rows.into_iter().map(convert::row_to_entity).collect()
    }

    async fn merge_entities(
        &self,
        ctx: &ScopeContext,
        keep_id: &str,
        merge_id: &str,
    ) -> Result<(), StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let keep = match self.get_entity(ctx, keep_id).await? {
            Some(e) => e,
            None => return Err(StoreError::NotFound),
        };
        let merge = match self.get_entity(ctx, merge_id).await? {
            Some(e) => e,
            None => return Err(StoreError::NotFound),
        };

        // Fold merge's canonical name + aliases into keep's aliases (deduplicated).
        let mut aliases = keep.aliases.clone();
        for a in merge
            .aliases
            .iter()
            .chain(std::iter::once(&merge.canonical_name))
        {
            if !aliases.contains(a) && a != &keep.canonical_name {
                aliases.push(a.clone());
            }
        }
        let keep_thing = id_thing(keep_id)?;
        let merge_thing = id_thing(merge_id)?;
        // Repoint fact.entities and relationship.from_ent/to_ent references, fold aliases, drop merge.
        self.db
            .query(self.scoped(&ctx.tenant, "BEGIN; \
                 UPDATE fact SET entities = array::union(array::complement(entities, [$mid]), [$kid]) \
                   WHERE $mid IN entities; \
                 UPDATE relationship SET from_ent = $kid WHERE from_ent = $mid; \
                 UPDATE relationship SET to_ent = $kid WHERE to_ent = $mid; \
                 UPDATE $keep SET aliases = $aliases; \
                 DELETE $merge; \
                 COMMIT;",)?)
            .bind(("mid", merge_id.to_string()))
            .bind(("kid", keep_id.to_string()))
            .bind(("keep", keep_thing))
            .bind(("merge", merge_thing))
            .bind(("aliases", aliases))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn put_relationship(&self, r: &Relationship) -> Result<(), StoreError> {
        if r.kind.is_empty() || r.kind.chars().count() > 128 {
            return Err(StoreError::Validation("kind length".into()));
        }
        if !(0.0..=1.0).contains(&r.confidence) {
            return Err(StoreError::Validation("confidence out of range".into()));
        }
        self.ensure_provisioned(&r.owner.tenant).await?;
        let thing = id_thing(&r.id)?;
        self.db
            .query(self.scoped(&r.owner.tenant, "UPSERT $id CONTENT $rec")?)
            .bind(("id", thing))
            .bind(("rec", Value::Object(convert::relationship_to_object(r))))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn get_relationship(
        &self,
        ctx: &ScopeContext,
        id: &str,
    ) -> Result<Option<Relationship>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let thing = id_thing(id)?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                format!("SELECT * FROM $id WHERE {}", novis_filter_clause()),
            )?)
            .bind(("id", thing))
            .bind(("ctenant", ctx.tenant.clone()))
            .bind(("cuser", ctx.user.clone()))
            .bind(("cteams", ctx.teams.clone()))
            .bind(("cnow", Utc::now()))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("read relationship: {e}")))?;
        match rows.into_iter().next() {
            Some(row) => Ok(Some(convert::row_to_relationship(row)?)),
            None => Ok(None),
        }
    }

    async fn end_relationship_validity(
        &self,
        ctx: &ScopeContext,
        id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        if self.get_relationship(ctx, id).await?.is_none() {
            return Err(StoreError::NotFound);
        }
        let thing = id_thing(id)?;
        self.db
            .query(self.scoped(
                &ctx.tenant,
                "UPDATE $id SET valid_to = $at WHERE valid_to IS NONE",
            )?)
            .bind(("id", thing))
            .bind(("at", surrealdb::types::Datetime::from(at)))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn put_source(&self, s: &Source) -> Result<(), StoreError> {
        if !(0.0..=1.0).contains(&s.trust_signal) {
            return Err(StoreError::Validation("trust_signal out of range".into()));
        }
        self.ensure_provisioned(&s.owner.tenant).await?;
        let thing = id_thing(&s.id)?;
        self.db
            .query(self.scoped(&s.owner.tenant, "UPSERT $id CONTENT $rec")?)
            .bind(("id", thing))
            .bind(("rec", Value::Object(convert::source_to_object(s))))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn get_source(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Source>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let thing = id_thing(id)?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                format!("SELECT * FROM $id WHERE {}", novis_filter_clause()),
            )?)
            .bind(("id", thing))
            .bind(("ctenant", ctx.tenant.clone()))
            .bind(("cuser", ctx.user.clone()))
            .bind(("cteams", ctx.teams.clone()))
            .bind(("cnow", Utc::now()))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("read source: {e}")))?;
        match rows.into_iter().next() {
            Some(row) => Ok(Some(convert::row_to_source(row)?)),
            None => Ok(None),
        }
    }

    async fn append_audit(&self, e: &AuditEntry) -> Result<(), StoreError> {
        self.ensure_provisioned(&e.tenant).await?;
        let thing = id_thing(&e.id)?;
        self.db
            .query(self.scoped(&e.tenant, "CREATE $id CONTENT $rec")?)
            .bind(("id", thing))
            .bind(("rec", Value::Object(convert::audit_to_object(e))))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn list_tenants(&self) -> Result<Vec<String>, StoreError> {
        // Root-level and deliberately NOT scoped — `INFO FOR ROOT` enumerates namespaces and belongs to
        // no tenant. So its rows are at slot 0, not slot 1: the RISK-012 pass shifted every scoped
        // statement's `take(n)` to `take(n+1)` and caught this one by mistake, which made the call
        // fail outright rather than return a wrong answer.
        let mut resp = self.db.query("INFO FOR ROOT").await.map_err(map_db_err)?;
        let info: Vec<Json> = resp
            .take(0)
            .map_err(|e| StoreError::Internal(format!("list tenants: {e}")))?;
        let mut out = Vec::new();
        if let Some(ns) = info
            .first()
            .and_then(|v| v.get("namespaces"))
            .and_then(|v| v.as_object())
        {
            out.extend(ns.keys().cloned());
        }
        Ok(out)
    }

    async fn scan_recent_episodes(
        &self,
        ctx: &ScopeContext,
        since: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT * FROM fact WHERE valid_to IS NONE AND memory_class = 'episodic' \
                 AND ingested_at >= $since LIMIT $limit",
            )?)
            .bind(("since", surrealdb::types::Datetime::from(since)))
            .bind(("limit", limit as i64))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("scan recent: {e}")))?;
        rows.into_iter().map(convert::row_to_fact).collect()
    }

    async fn scan_contradiction_candidates(
        &self,
        ctx: &ScopeContext,
        limit: u32,
    ) -> Result<Vec<(Fact, Fact)>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        // Currently-valid facts; pair those sharing >=1 entity. The pairing is done client-side over
        // a bounded window to keep the query simple and parameterised.
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT * FROM fact WHERE valid_to IS NONE LIMIT $limit",
            )?)
            .bind(("limit", (limit.saturating_mul(2)).max(2) as i64))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("scan contradiction: {e}")))?;
        let facts: Vec<Fact> = rows
            .into_iter()
            .map(convert::row_to_fact)
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
        self.ensure_provisioned(&ctx.tenant).await?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT * FROM fact WHERE valid_to IS NONE AND salience < $floor \
                 ORDER BY last_recalled_at ASC, ingested_at ASC LIMIT $limit",
            )?)
            .bind(("floor", salience_floor))
            .bind(("limit", limit as i64))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("scan decay: {e}")))?;
        rows.into_iter().map(convert::row_to_fact).collect()
    }

    async fn scan_reembed_candidates(
        &self,
        ctx: &ScopeContext,
        current_model_version: &str,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        let mut resp = self
            .db
            .query(self.scoped(
                &ctx.tenant,
                "SELECT * FROM fact WHERE valid_to IS NONE \
                 AND (embedding_model != $current OR embedding IS NONE) LIMIT $limit",
            )?)
            .bind(("current", current_model_version.to_string()))
            .bind(("limit", limit as i64))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("scan reembed: {e}")))?;
        rows.into_iter().map(convert::row_to_fact).collect()
    }

    async fn update_fact_maintenance_fields(
        &self,
        ctx: &ScopeContext,
        f: &Fact,
    ) -> Result<(), StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        if self.get_fact(ctx, &f.id).await?.is_none() {
            return Err(StoreError::NotFound);
        }
        let thing = id_thing(&f.id)?;
        self.db
            .query(self.scoped(
                &ctx.tenant,
                "UPDATE $id SET confidence = $c, salience = $s, stability = $st, \
                 last_recalled_at = $lr, supersedes = $sup, superseded_by = $supby",
            )?)
            .bind(("id", thing))
            .bind(("c", f.confidence))
            .bind(("s", f.salience))
            .bind(("st", f.stability))
            .bind((
                "lr",
                match f.last_recalled_at {
                    Some(t) => Value::Datetime(surrealdb::types::Datetime::from(t)),
                    None => Value::None,
                },
            ))
            .bind((
                "sup",
                match &f.supersedes {
                    Some(x) => Value::String(x.clone()),
                    None => Value::None,
                },
            ))
            .bind((
                "supby",
                match &f.superseded_by {
                    Some(x) => Value::String(x.clone()),
                    None => Value::None,
                },
            ))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
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
        self.ensure_provisioned(&ctx.tenant).await?;
        let thing = id_thing(fact_id)?;
        self.db
            .query(self.scoped(
                &ctx.tenant,
                "UPDATE $id SET embedding = $v, embedding_model = $m",
            )?)
            .bind(("id", thing))
            .bind(("v", vector.to_vec()))
            .bind(("m", model_version.to_string()))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn ensure_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError> {
        self.migrator().migrate_up(tenant).await?;
        Ok(())
    }

    async fn erase_tenant(&self, tenant: &str) -> Result<TenantErasureProof, StoreError> {
        validate_tenant(tenant)?;
        // Provision first, so a tenant that was never written erases to a zero-count proof rather
        // than failing on a table that does not exist yet.
        self.ensure_provisioned(tenant).await?;
        let namespace = self.placement(tenant).namespace;

        let mut removed: BTreeMap<String, u64> = BTreeMap::new();
        let mut total_removed: u64 = 0;
        for (table, field) in ERASURE_SET {
            let n = self.erase_table_rows(tenant, table, field).await?;
            total_removed += n;
            removed.insert((*table).to_string(), n);
        }

        // Verification: an incomplete erasure never reports success (SA-DELETE-01, applied to the
        // tenant path). Re-count on the same predicate; anything left is a partial erasure.
        let mut remaining: u64 = 0;
        for (table, field) in ERASURE_SET {
            remaining += self.count_table_rows(tenant, table, field).await?;
        }
        if remaining > 0 {
            // Saturate rather than truncate: the variant carries u32 and a tenant's row count is not
            // bounded by it, and a wrapped number in a failure report is worse than a clamped one.
            let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
            return Err(StoreError::PartialDelete {
                removed: clamp(total_removed.saturating_sub(remaining)),
                expected: clamp(total_removed),
            });
        }

        let digest = sha256_hex(&erasure_digest_input(tenant, &namespace, &removed));
        tracing::warn!(
            target: "memory",
            total_removed,
            tenant_present = crate::obs::log::tenant_present(Some(tenant)),
            "store.erase_tenant"
        );
        Ok(TenantErasureProof {
            erased_at: Utc::now(),
            tenant: tenant.to_string(),
            namespace,
            removed,
            total_removed,
            digest,
        })
    }

    async fn drop_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError> {
        validate_tenant(tenant)?;
        // `REMOVE NAMESPACE` is bounded by nothing: it removes the namespace and everything in it.
        // That erases one tenant only where the namespace holds exactly one, so refuse anywhere else
        // rather than destroy a co-resident tenant's data on the erasure path (ADR-018, RISK-021).
        ensure_namespace_exclusive(self.profile, tenant, &self.placement(tenant))?;
        tracing::warn!(target: "memory", "store.drop_ns");
        self.db
            // Root-level DDL: deliberately NOT namespace-scoped. Removing a namespace cannot run
            // inside it, and `validate_tenant` above is the gate on the interpolated identifier.
            .query(format!("REMOVE NAMESPACE IF EXISTS {tenant}"))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    async fn ready(&self) -> Result<(), StoreError> {
        // A live connection answers a trivial query; the per-tenant vector-index dimension is checked
        // at migration time (the HNSW DDL embeds MEMORY_EMBED_DIM via the Migrator). A failure here is
        // a lost connection.
        self.db
            // Connection liveness only — no tenant, so deliberately not namespace-scoped.
            .query("RETURN 1")
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }
}

impl Store {
    /// Bind the shared recall parameters (read filter + metadata filters + validity) onto a query.
    fn bind_recall<'q>(
        &self,
        mut query: surrealdb::method::Query<'q, Any>,
        ctx: &ScopeContext,
        q: &StageOneQuery,
    ) -> surrealdb::method::Query<'q, Any> {
        query = query
            .bind(("ctenant", ctx.tenant.clone()))
            .bind(("cuser", ctx.user.clone()))
            .bind(("cteams", ctx.teams.clone()))
            .bind(("cnow", Utc::now()));
        if let Some(mc) = q.filters.memory_class {
            query = query.bind(("mclass", convert::memory_class_str(mc).to_string()));
        }
        if let Some(v) = q.filters.visibility {
            query = query.bind(("vis", convert::visibility_str(v).to_string()));
        }
        if let Some(e) = &q.filters.entity {
            query = query.bind(("entity", e.clone()));
        }
        query = bind_valid_at(query, q.filters.valid_at);
        query
    }
}

/// C8 idempotency-record persistence. These concrete methods (not on the `MemoryStore` trait) back
/// the HTTP API Edge's idempotency layer: the edge stores one outcome per (tenant, user, route, key)
/// and replays it verbatim within the TTL window (SA-IDEM-01). Keys and values are bound parameters,
/// never string-interpolated (sql-safety). The table is defined by migration 0005 and lives inside the
/// tenant namespace alongside `audit_log`.
impl Store {
    /// The embedding dimension this store was opened with — the dimension baked into every tenant's
    /// HNSW vector-index DDL by the Migrator. `/readyz` (C8) compares this against `MEMORY_EMBED_DIM`
    /// to satisfy SA-EMBED-01.
    pub fn index_embed_dim(&self) -> u32 {
        self.embed_dim
    }

    /// Look up a stored idempotency outcome for `(tenant, user, route, key)`. Returns the
    /// `(status_code, response_body)` of a non-expired record, or `None` on a miss (no row, or a row
    /// whose `expires_at` is at/before now — a read past expiry is a miss; the handler re-runs).
    pub async fn idempotency_get(
        &self,
        tenant: &str,
        user: &str,
        route: &str,
        key: &str,
    ) -> Result<Option<(i64, Json)>, StoreError> {
        self.ensure_provisioned(tenant).await?;
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                "SELECT status_code, response_body FROM idempotency_record \
                 WHERE tenant = $tenant AND user = $user AND route = $route \
                   AND idempotency_key = $key AND expires_at > time::now() LIMIT 1",
            )?)
            .bind(("tenant", tenant.to_string()))
            .bind(("user", user.to_string()))
            .bind(("route", route.to_string()))
            .bind(("key", key.to_string()))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("idempotency_get: {e}")))?;
        match rows.into_iter().next() {
            Some(row) => {
                let status = row
                    .get("status_code")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| StoreError::Internal("idempotency_get: status_code".into()))?;
                let body = row.get("response_body").cloned().unwrap_or(Json::Null);
                Ok(Some((status, body)))
            }
            None => Ok(None),
        }
    }

    /// Persist an idempotency outcome with `expires_at = now + ttl`. Idempotent on the unique
    /// `(tenant, user, route, key)` index: an existing row is overwritten (a re-run after expiry), so
    /// the stored outcome is always the latest handler result for the key. The record id is a deterministic
    /// `idempotency_record:<uuidv5(tenant|user|route|key)>` so a re-run targets the same row.
    #[allow(clippy::too_many_arguments)]
    pub async fn idempotency_put(
        &self,
        tenant: &str,
        user: &str,
        route: &str,
        key: &str,
        status_code: i64,
        body: &Json,
        ttl_secs: u32,
    ) -> Result<(), StoreError> {
        self.ensure_provisioned(tenant).await?;
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(i64::from(ttl_secs));
        // Deterministic record key so a replay-after-expiry overwrites the same row rather than racing
        // the UNIQUE index. The components are joined with a separator that cannot appear in a tenant or
        // route token, keeping the derivation injective.
        let seed = format!("{tenant}\u{1f}{user}\u{1f}{route}\u{1f}{key}");
        let rec_key = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, seed.as_bytes()).to_string();
        let id = format!("idempotency_record:{rec_key}");
        let thing = id_thing(&id)?;

        let mut obj = surrealdb::types::Object::new();
        obj.insert("tenant", Value::String(tenant.to_string()));
        obj.insert("user", Value::String(user.to_string()));
        obj.insert("route", Value::String(route.to_string()));
        obj.insert("idempotency_key", Value::String(key.to_string()));
        obj.insert("response_body", json_to_value(body));
        obj.insert(
            "status_code",
            Value::Number(surrealdb::types::Number::Int(status_code)),
        );
        obj.insert(
            "created_at",
            Value::Datetime(surrealdb::types::Datetime::from(now)),
        );
        obj.insert(
            "expires_at",
            Value::Datetime(surrealdb::types::Datetime::from(expires_at)),
        );

        self.db
            .query(self.scoped(tenant, "UPSERT $id CONTENT $rec")?)
            .bind(("id", thing))
            .bind(("rec", Value::Object(obj)))
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }
}

/// Convert a `serde_json::Value` into a native SurrealDB value (used for the FLEXIBLE response_body
/// field, which stores an arbitrary nested Success payload).
fn json_to_value(v: &Json) -> Value {
    use surrealdb::types::SurrealValue;
    v.clone().into_value()
}

/// Bind the `$valid_at` parameter when a bi-temporal as-of filter is set.
fn bind_valid_at(
    query: surrealdb::method::Query<'_, Any>,
    valid_at: Option<DateTime<Utc>>,
) -> surrealdb::method::Query<'_, Any> {
    match valid_at {
        Some(at) => query.bind(("valid_at", surrealdb::types::Datetime::from(at))),
        None => query,
    }
}

/// Whether a fact row currently carries a non-null embedding vector.
///
/// A method on `Store` rather than a free function so it resolves placement through the same seam as
/// every other statement — a free function would have to be handed a namespace, which is exactly the
/// coupling `Placement` exists to remove.
impl Store {
    /// Run the stage-1 vector signal under `strategy` and return `(fact_id, similarity)` in rank
    /// order, best first. `fact_id` is the bare key, as the SQL projects it.
    ///
    /// **The single definition of the vector leg.** `memory` calls it with the profile's strategy and
    /// folds the result into its multi-signal score map; `recall_vector_only` calls it with a strategy
    /// the caller names. One definition rather than two call sites is deliberate: a benchmark whose
    /// query has drifted from the one production runs reports numbers that are worse than missing,
    /// because they get quoted. There is no second copy here to drift from.
    async fn vector_leg(
        &self,
        ctx: &ScopeContext,
        q: &StageOneQuery,
        strategy: VectorStrategy,
    ) -> Result<Vec<(String, f64)>, StoreError> {
        if q.query_vector.is_empty() {
            return Ok(Vec::new());
        }
        let k = q.stage1_k.max(1) as i64;
        let (read, hint) = vector_read_filter(self.profile);
        let sql = vector_signal_sql(
            strategy,
            k,
            read,
            hint,
            validity_clause(q.filters.valid_at),
            &meta_clause(q),
        );
        let mut resp = self
            .bind_recall(self.db.query(self.scoped(&ctx.tenant, &sql)?), ctx, q)
            .bind(("qv", q.query_vector.clone()))
            .bind(("k", k))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("vector signal: {e}")))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            if let (Some(fid), Some(dist)) = (
                r.get("fid").and_then(|v| v.as_str()),
                r.get("dist").and_then(|v| v.as_f64()),
            ) {
                // Both strategies project a distance so the caller needs no knowledge of which ran;
                // the clamp recovers the same [0,1] similarity from either.
                out.push((fid.to_string(), (1.0 - dist).clamp(0.0, 1.0)));
            }
        }
        Ok(out)
    }

    /// The stage-1 vector signal alone, under an explicitly named strategy — the measurement seam the
    /// re-pointed benchmark times (plan 06 Phase 1).
    ///
    /// Two things it deliberately does, both of which the measurement needs. It runs **one** signal, so
    /// an NFR-P3 reading is not inflated by the keyword and graph legs or by the per-candidate
    /// `get_fact` resolution `memory` does afterwards. And it takes the strategy as a **parameter**
    /// rather than reading it from the profile, so both strategies can be timed against one store
    /// holding one corpus — comparing them across two stores would measure the data, not the strategy.
    ///
    /// `pub(crate)` and no wider (rule C12): this is reachable from `src/bench` and from nowhere
    /// outside the crate.
    // Consumed by the relevance, latency and co-residency legs (plan 06 Phases 3-5); until those land
    // its only caller is the seam-agreement test, which a non-test build cannot see.
    #[allow(dead_code)]
    pub(crate) async fn recall_vector_only(
        &self,
        ctx: &ScopeContext,
        q: &StageOneQuery,
        strategy: VectorStrategy,
    ) -> Result<Vec<(String, f64)>, StoreError> {
        self.ensure_provisioned(&ctx.tenant).await?;
        self.vector_leg(ctx, q, strategy).await
    }

    async fn fact_has_embedding(&self, tenant: &str, id: &str) -> Result<bool, StoreError> {
        let thing = id_thing(id)?;
        let mut resp = self
            .db
            .query(self.scoped(tenant, "SELECT embedding FROM $id")?)
            .bind(("id", thing))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("embedding probe: {e}")))?;
        Ok(rows
            .first()
            .and_then(|r| r.get("embedding"))
            .map(|v| !v.is_null())
            .unwrap_or(false))
    }

    /// Count the rows of one table that carry `tenant` in `field`.
    ///
    /// `table` and `field` come from `ERASURE_SET` — compile-time constants, never caller input — so
    /// interpolating them is not a caller-controlled path; the tenant is bound (sql-safety layer).
    async fn count_table_rows(
        &self,
        tenant: &str,
        table: &str,
        field: &str,
    ) -> Result<u64, StoreError> {
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                format!("SELECT count() FROM {table} WHERE {field} = $t GROUP ALL"),
            )?)
            .bind(("t", tenant.to_string()))
            .await
            .map_err(map_db_err)?;
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("count {table}: {e}")))?;
        Ok(count_from_rows(&rows))
    }

    /// Count, then delete, the rows of one table that carry `tenant` in `field`, returning the count
    /// removed. Both statements travel in the same batch as the `USE` that selects the namespace
    /// (RISK-012), so a concurrent task cannot move the namespace between the count and the delete,
    /// and the proof cannot report a number the delete did not remove.
    async fn erase_table_rows(
        &self,
        tenant: &str,
        table: &str,
        field: &str,
    ) -> Result<u64, StoreError> {
        let mut resp = self
            .db
            .query(self.scoped(
                tenant,
                format!(
                    "SELECT count() FROM {table} WHERE {field} = $t GROUP ALL; \
                     DELETE {table} WHERE {field} = $t"
                ),
            )?)
            .bind(("t", tenant.to_string()))
            .await
            .map_err(map_db_err)?;
        // Read the count before `check()` consumes the response; `check()` then surfaces a failure in
        // either statement, so a delete that errored never reports a removed count.
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("count {table}: {e}")))?;
        let n = count_from_rows(&rows);
        resp.check().map_err(map_db_err)?;
        Ok(n)
    }
}

/// Read the number out of a `SELECT count() … GROUP ALL` result set. `GROUP ALL` yields one row on a
/// non-empty match and no rows at all on an empty one, so an absent row is zero.
fn count_from_rows(rows: &[Json]) -> u64 {
    rows.first()
        .and_then(|r| r.get("count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

/// Refuse an unbounded namespace-level destructive operation unless `tenant` has `placement`'s
/// namespace to itself (ADR-018).
///
/// A free function so the refusal is reachable in a unit test: no profile maps two tenants onto one
/// namespace yet, so the store cannot be driven into this branch through its own constructor, and an
/// untested refusal is indistinguishable from an absent one.
fn ensure_namespace_exclusive(
    profile: MemoryProfile,
    tenant: &str,
    placement: &Placement,
) -> Result<(), StoreError> {
    if placement::namespace_is_exclusive(profile, tenant, placement) {
        return Ok(());
    }
    tracing::warn!(target: "memory", "store.drop_ns.refused");
    Err(StoreError::NamespaceNotExclusive {
        tenant: tenant.to_string(),
        namespace: placement.namespace.clone(),
    })
}

/// Every table in a namespace that carries a tenant, with the field that carries it — the erasure
/// predicate set (ADR-018). It must stay exhaustive: a table added to the schema without an entry
/// here leaves that tenant's rows behind after `erase_tenant` reports success.
///
/// `schema_migrations` is deliberately absent. It records the namespace's schema version, not any
/// tenant's data, and under a co-resident topology it is shared.
const ERASURE_SET: &[(&str, &str)] = &[
    ("audit_log", "scope.tenant"),
    ("dead_letter", "scope.tenant"),
    ("entity", "owner.tenant"),
    ("fact", "owner.tenant"),
    ("grant", "grantor.tenant"),
    ("idempotency_record", "tenant"),
    ("maintenance_state", "tenant"),
    ("quarantine", "owner.tenant"),
    ("relationship", "owner.tenant"),
    ("source", "owner.tenant"),
    ("work_job", "scope.tenant"),
];

/// The lines a `TenantErasureProof` digest is taken over: the tenant, the namespace it was erased
/// from, and one `<table>=<count>` line per table in key order (`BTreeMap` supplies the order).
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

/// SHA-256 hex over the newline-joined, already-sorted ids (SA-DELETE-01 digest).
fn sha256_hex(sorted_ids: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sorted_ids.join("\n").as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Defensive in-process read-filter check, reusing the §2C.3 pure helper. Used in unit tests to
/// confirm the store's SurrealQL filter agrees with the canonical rule.
#[allow(dead_code)]
fn local_can_read(
    ctx: &ScopeContext,
    owner: &ScopeRef,
    vis: crate::types::domain::Visibility,
) -> bool {
    can_read(ctx, owner, vis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::domain::{MemoryClass, Visibility};
    use crate::types::ports::RecallFilters;
    use crate::types::scope::OpSet;

    const DIM: u32 = 8;

    fn scope_ref(tenant: &str, team: Option<&str>, user: &str) -> ScopeRef {
        ScopeRef {
            tenant: tenant.into(),
            team: team.map(Into::into),
            user: user.into(),
        }
    }

    fn ctx(tenant: &str, user: &str, teams: &[&str]) -> ScopeContext {
        ScopeContext {
            tenant: tenant.into(),
            teams: teams.iter().map(|t| t.to_string()).collect(),
            user: user.into(),
            token_jti: "jti".into(),
            allowed_ops: OpSet {
                read: true,
                write: true,
                forget: true,
            },
            client: None,
            fact_authority: None,
            correlation_id: "c-test".into(),
        }
    }

    fn sample_fact(id: &str, owner: ScopeRef, vis: Visibility) -> Fact {
        Fact {
            id: id.into(),
            content: serde_json::json!({"subject": "team:alpha", "predicate": "owns", "object": "table:orders"}),
            entities: vec!["entity:e1".into()],
            source_id: None,
            memory_class: MemoryClass::Semantic,
            fact_class: None,
            visibility: vis,
            owner,
            valid_from: Utc::now(),
            valid_to: None,
            ingested_at: Utc::now(),
            confidence: 0.9,
            salience: 0.7,
            stability: 1.0,
            pii_review: false,
            supersedes: None,
            superseded_by: None,
            derived_from: vec![],
            last_recalled_at: None,
        }
    }

    async fn store() -> Store {
        Store::new_in_memory(DIM).await.expect("in-memory store")
    }

    #[tokio::test]
    async fn migrate_up_is_idempotent_and_dry_run_executes_nothing() {
        let s = store().await;
        let m = s.migrator();
        // 0001_init is the squashed greenfield schema (C1 store + audit, C2 queue, C4 quarantine,
        // C7 maintenance_state, C8 idempotency_record); 0002 corrects the graph-leg entity index
        // (RISK-018), 0003 adds the lineage index (FU-025), 0004 defines the `grant` table backing
        // the consent-grant arm of the fact read filter (SA-GRANT-01), and 0005 adds the `client` and
        // `fact_class` columns the fact-class refusal is audited with (SA-AUTHZ-04), and 0006 adds
        // `fact_class` to the SCHEMAFULL `quarantine` table so a held write records what it claimed to
        // be (SA-AUTHZ-09). A fresh namespace migrates through all six in one pass.
        const LATEST: u32 = 6;
        assert_eq!(m.current_version("acme").await.unwrap(), 0);
        assert_eq!(m.migrate_up("acme").await.unwrap(), LATEST);
        assert_eq!(m.current_version("acme").await.unwrap(), LATEST);
        // Second migrate_up is a no-op (still at the latest version).
        assert_eq!(m.migrate_up("acme").await.unwrap(), LATEST);
        // dry_run on an up-to-date namespace returns nothing and does not change the version.
        assert!(m.dry_run("acme").await.unwrap().is_empty());
        assert_eq!(m.current_version("acme").await.unwrap(), LATEST);
    }

    #[tokio::test]
    async fn put_then_get_round_trips() {
        let s = store().await;
        let owner = scope_ref("acme", Some("alpha"), "u-sarah");
        let f = sample_fact("fact:f1", owner, Visibility::TeamShared);
        s.put_fact(&f).await.unwrap();
        let c = ctx("acme", "u-sarah", &["alpha"]);
        let got = s
            .get_fact(&c, "fact:f1")
            .await
            .unwrap()
            .expect("fact present");
        assert_eq!(got.id, "fact:f1");
        assert_eq!(got.confidence, 0.9);
        assert_eq!(got.entities, vec!["entity:e1".to_string()]);
    }

    #[tokio::test]
    async fn score_out_of_range_is_rejected_with_no_write() {
        let s = store().await;
        let owner = scope_ref("acme", None, "u-sarah");
        let mut f = sample_fact("fact:bad", owner, Visibility::UserPrivate);
        f.confidence = 1.4;
        let err = s.put_fact(&f).await.unwrap_err();
        assert!(matches!(err, StoreError::Validation(ref m) if m.contains("range")));
        // No record was written.
        let c = ctx("acme", "u-sarah", &[]);
        assert!(s.get_fact(&c, "fact:bad").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_out_of_scope_returns_none() {
        let s = store().await;
        // user-private fact owned by sarah.
        let owner = scope_ref("acme", None, "u-sarah");
        let f = sample_fact("fact:priv", owner, Visibility::UserPrivate);
        s.put_fact(&f).await.unwrap();
        // a different same-tenant user cannot read it.
        let bob = ctx("acme", "u-bob", &[]);
        assert!(s.get_fact(&bob, "fact:priv").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn supersede_ends_validity_without_deleting() {
        let s = store().await;
        let owner = scope_ref("acme", None, "u-sarah");
        s.put_fact(&sample_fact(
            "fact:old",
            owner.clone(),
            Visibility::UserPrivate,
        ))
        .await
        .unwrap();
        s.put_fact(&sample_fact("fact:new", owner, Visibility::UserPrivate))
            .await
            .unwrap();
        let c = ctx("acme", "u-sarah", &[]);
        let at = DateTime::parse_from_rfc3339("2026-01-10T12:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        s.supersede(&c, "fact:old", "fact:new", at).await.unwrap();

        let old = s
            .get_fact(&c, "fact:old")
            .await
            .unwrap()
            .expect("old retained");
        assert_eq!(old.valid_to, Some(at));
        assert_eq!(old.superseded_by.as_deref(), Some("fact:new"));
        let new = s
            .get_fact(&c, "fact:new")
            .await
            .unwrap()
            .expect("new present");
        assert_eq!(new.supersedes.as_deref(), Some("fact:old"));
    }

    #[tokio::test]
    async fn hard_delete_returns_proof_and_removes_derived() {
        let s = store().await;
        let owner = scope_ref("acme", None, "u-sarah");
        s.put_fact(&sample_fact(
            "fact:base",
            owner.clone(),
            Visibility::UserPrivate,
        ))
        .await
        .unwrap();
        // two consolidated insights derived from base.
        for iid in ["fact:i1", "fact:i2"] {
            let mut insight = sample_fact(iid, owner.clone(), Visibility::UserPrivate);
            insight.memory_class = MemoryClass::Consolidated;
            insight.derived_from = vec!["fact:base".into()];
            s.put_fact(&insight).await.unwrap();
        }
        let c = ctx("acme", "u-sarah", &[]);
        let proof = s.hard_delete(&c, "fact:base").await.unwrap();
        assert_eq!(proof.record_id, "fact:base");
        let mut derived = proof.derived_removed.clone();
        derived.sort();
        assert_eq!(derived, vec!["fact:i1".to_string(), "fact:i2".to_string()]);
        // digest = sha256 of sorted removed ids.
        let mut all = vec!["fact:base".to_string(), "fact:i1".into(), "fact:i2".into()];
        all.sort();
        assert_eq!(proof.digest, sha256_hex(&all));
        assert!(s.get_fact(&c, "fact:base").await.unwrap().is_none());
        assert!(s.get_fact(&c, "fact:i1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn recall_returns_candidate_by_vector_and_keyword() {
        let s = store().await;
        let owner = scope_ref("acme", Some("alpha"), "u-sarah");
        let f = sample_fact("fact:r1", owner, Visibility::TeamShared);
        s.put_fact(&f).await.unwrap();
        let c = ctx("acme", "u-sarah", &["alpha"]);
        s.set_fact_embedding(&c, "fact:r1", &[0.1_f32; DIM as usize], "m1")
            .await
            .unwrap();
        let q = StageOneQuery {
            query_vector: vec![0.1_f32; DIM as usize],
            keyword_terms: vec!["orders".into()],
            filters: RecallFilters::default(),
            scope: c.clone(),
            stage1_k: 50,
        };
        let cands = s.recall(&c, &q).await.unwrap();
        assert_eq!(cands.len(), 1, "expected one candidate");
        let cand = &cands[0];
        assert_eq!(cand.fact_id, "fact:r1");
        assert!((0.0..=1.0).contains(&cand.semantic_score));
        assert!((0.0..=1.0).contains(&cand.keyword_score));
    }

    #[tokio::test]
    async fn cross_tenant_isolation_on_recall() {
        let s = store().await;
        // tenant-shared fact in acme.
        let owner = scope_ref("acme", None, "u-sarah");
        let f = sample_fact("fact:x", owner, Visibility::TenantShared);
        s.put_fact(&f).await.unwrap();
        let acme = ctx("acme", "u-sarah", &[]);
        s.set_fact_embedding(&acme, "fact:x", &[0.2_f32; DIM as usize], "m1")
            .await
            .unwrap();
        // globex caller must see nothing (different namespace).
        let globex = ctx("globex", "u-other", &[]);
        let q = StageOneQuery {
            query_vector: vec![0.2_f32; DIM as usize],
            keyword_terms: vec!["orders".into()],
            filters: RecallFilters::default(),
            scope: globex.clone(),
            stage1_k: 50,
        };
        let cands = s.recall(&globex, &q).await.unwrap();
        assert!(cands.is_empty(), "globex must not see acme facts");
    }

    /// RISK-009: the maintenance scope reads (and so can mutate) a user-private fact that a regular
    /// non-owner scope cannot see, while a cross-tenant maintenance scope still sees nothing.
    #[tokio::test]
    async fn maintenance_scope_reaches_user_private_facts_within_the_tenant_only() {
        let s = store().await;
        let owner = scope_ref("acme", None, "u-sarah");
        let f = sample_fact("fact:mp", owner, Visibility::UserPrivate);
        s.put_fact(&f).await.unwrap();

        // A regular, non-owner caller in the same tenant cannot see a user-private fact.
        let other = ctx("acme", "u-bob", &[]);
        assert!(s.get_fact(&other, "fact:mp").await.unwrap().is_none());

        // The maintenance scope (empty user + "maintenance" jti) reaches it (RISK-009 fix).
        let maint = ScopeContext {
            tenant: "acme".into(),
            teams: vec![],
            user: String::new(),
            token_jti: "maintenance".into(),
            allowed_ops: OpSet {
                read: true,
                write: true,
                forget: true,
            },
            client: None,
            fact_authority: None,
            correlation_id: "c-maint".into(),
        };
        assert!(
            s.get_fact(&maint, "fact:mp").await.unwrap().is_some(),
            "maintenance scope must reach a user-private fact"
        );
        // Maintenance can end its validity (was a silent no-op before the fix).
        s.end_validity(&maint, "fact:mp", Utc::now()).await.unwrap();
        assert!(
            s.get_fact(&maint, "fact:mp")
                .await
                .unwrap()
                .unwrap()
                .valid_to
                .is_some(),
            "maintenance end_validity must take effect on a user-private fact"
        );

        // Tenant isolation stays structural: a maintenance scope for another tenant sees nothing.
        let maint_globex = ScopeContext {
            tenant: "globex".into(),
            teams: vec![],
            user: String::new(),
            token_jti: "maintenance".into(),
            allowed_ops: OpSet {
                read: true,
                write: true,
                forget: true,
            },
            client: None,
            fact_authority: None,
            correlation_id: "c-maint2".into(),
        };
        assert!(
            s.get_fact(&maint_globex, "fact:mp")
                .await
                .unwrap()
                .is_none(),
            "maintenance must not cross tenants"
        );
    }

    /// The profile does not move any tenant's data.
    ///
    /// Stage 1 makes the profile change *how* a namespace is searched; it must not change *which*
    /// namespace is searched. That is Stage 2's job, and keeping the two apart is what allows the
    /// retrieval change to ship on its own.
    #[test]
    fn profile_does_not_yet_change_placement() {
        for profile in [MemoryProfile::Collective, MemoryProfile::Individual] {
            let p = placement::resolve(profile, "acme", DB_NAME);
            assert_eq!(
                p.namespace, "acme",
                "{profile}: namespace must still be the tenant"
            );
            assert_eq!(
                p.database, DB_NAME,
                "{profile}: database must still be the default"
            );
        }
    }

    /// The statement the default profile emits, pinned exactly.
    ///
    /// This is the regression guard the staged delivery rests on: a stage may add a strategy, but it
    /// may not quietly reword the one every existing deployment runs. Asserting the exact string
    /// rather than a property is deliberate — a subtle rewrite is precisely the kind of change that
    /// passes a looser test and alters production relevance.
    ///
    /// Deliberately changed once since Stage 1, by the RISK-017 fix, which added the
    /// `owner.tenant = $ctenant` conjunct. That is a *security* change rather than a retrieval one:
    /// today it selects the same rows, because a namespace holds one tenant and every fact in it
    /// already carries that tenant. Updating this expectation is the point at which such a change has
    /// to be noticed and justified rather than absorbed.
    #[test]
    fn the_collective_statement_is_pinned() {
        // Obtained through `vector_read_filter`, which is how production builds it. The test
        // previously called `fact_filter_clause()` directly; that reached past the function which
        // chooses the vector filter, and stopped mirroring production the moment the two diverged.
        // They diverged when the consent-grant arm was added to the non-vector filter and
        // deliberately withheld from the vector one (SA-GRANT-08). The pinned string below is
        // unchanged, which is the assertion that matters: the collective vector statement is the
        // same statement every existing deployment already runs.
        let (read, hint) = vector_read_filter(MemoryProfile::Collective);
        let sql = vector_signal_sql(
            VectorStrategy::for_profile(MemoryProfile::Collective),
            50,
            read,
            hint,
            validity_clause(None),
            "",
        );
        assert_eq!(
            sql,
            "SELECT meta::id(id) AS fid, vector::distance::knn() AS dist \
                 FROM fact WHERE embedding <|200,200|> $qv AND (owner.tenant = $ctenant AND (owner.user = $cuser \
     OR (visibility = 'team-shared' AND owner.team IN $cteams) \
     OR visibility = 'tenant-shared')) AND valid_to IS NONE \
                 ORDER BY dist ASC LIMIT $k",
            "the collective statement is pinned; changing it must be deliberate and explained here"
        );
    }

    // --- Consent grants (SA-GRANT-01, SA-GRANT-08) ------------------------------------------------

    /// SA-GRANT-01: the grant arm is inside the query, not applied after it.
    ///
    /// Asserted on the fragment itself rather than on behaviour, because the property being protected
    /// is *where* the check happens. A grant evaluated after the fetch would still pass a behavioural
    /// test while having already disclosed the row it then hides.
    #[test]
    fn grant_arm_is_inside_the_fact_read_filter() {
        let f = fact_filter_clause();
        assert!(
            f.contains("SELECT VALUE type::record(fact) FROM grant"),
            "the grant arm must be a sub-select inside the filter, and must convert `grant.fact` \
             to a record id — the bare string never matches `fact.id` (FU-026): {f}"
        );
        assert!(
            f.contains("revoked_at IS NONE"),
            "revocation must be in-query: {f}"
        );
        assert!(
            f.contains("valid_from <= $cnow") && f.contains("valid_to > $cnow"),
            "the validity window must be in-query, bound from one clock read: {f}"
        );
        // The tenant conjunct stays outside the disjunction: a grant never crosses a tenant.
        assert!(
            f.starts_with("owner.tenant = $ctenant AND ("),
            "the tenant conjunct must bind the whole disjunction: {f}"
        );
    }

    /// SA-GRANT-01, behavioural: a grantee holding an active grant can actually read the fact.
    ///
    /// This test exists because its absence let a dead arm ship. The three tests written alongside the
    /// grant model asserted on the SQL fragment string and on `Grant::is_active_at` in Rust — neither
    /// runs the query against data, so an arm that never matches passed all three. Found by the
    /// `recall` session against a live store (FU-026).
    #[tokio::test]
    async fn a_grantee_can_read_the_granted_fact_and_nothing_else() {
        let s = store().await;
        let owner = scope_ref("acme", None, "u-owner");
        s.put_fact(&sample_fact(
            "fact:granted",
            owner.clone(),
            Visibility::UserPrivate,
        ))
        .await
        .unwrap();
        s.put_fact(&sample_fact(
            "fact:private",
            owner.clone(),
            Visibility::UserPrivate,
        ))
        .await
        .unwrap();

        let now = Utc::now();
        s.put_grant_for_test(
            "acme",
            "grant:g1",
            "fact:granted",
            &owner,
            "u-grantee",
            now - chrono::Duration::hours(1),
            now + chrono::Duration::hours(1),
        )
        .await
        .unwrap();

        let grantee = ctx("acme", "u-grantee", &[]);
        assert!(
            s.get_fact(&grantee, "fact:granted")
                .await
                .unwrap()
                .is_some(),
            "an active grant must admit the read; a type mismatch between grant.fact and fact.id \
             makes the arm inert and this is the assertion that catches it"
        );
        assert!(
            s.get_fact(&grantee, "fact:private")
                .await
                .unwrap()
                .is_none(),
            "the grant must not widen past the one fact it names (SA-GRANT-03)"
        );

        // The owner is unaffected in both directions.
        let own = ctx("acme", "u-owner", &[]);
        assert!(s.get_fact(&own, "fact:granted").await.unwrap().is_some());
        assert!(s.get_fact(&own, "fact:private").await.unwrap().is_some());
    }

    /// SA-GRANT-02 / SA-GRANT-04, behavioural: revocation and expiry both close the read, in-query.
    #[tokio::test]
    async fn revoked_and_expired_grants_stop_admitting_reads() {
        let s = store().await;
        let owner = scope_ref("acme", None, "u-owner");
        s.put_fact(&sample_fact(
            "fact:a",
            owner.clone(),
            Visibility::UserPrivate,
        ))
        .await
        .unwrap();
        s.put_fact(&sample_fact(
            "fact:b",
            owner.clone(),
            Visibility::UserPrivate,
        ))
        .await
        .unwrap();
        let now = Utc::now();

        // Already expired.
        s.put_grant_for_test(
            "acme",
            "grant:expired",
            "fact:a",
            &owner,
            "u-grantee",
            now - chrono::Duration::hours(2),
            now - chrono::Duration::hours(1),
        )
        .await
        .unwrap();
        // Active, then revoked.
        s.put_grant_for_test(
            "acme",
            "grant:live",
            "fact:b",
            &owner,
            "u-grantee",
            now - chrono::Duration::hours(1),
            now + chrono::Duration::hours(1),
        )
        .await
        .unwrap();

        let grantee = ctx("acme", "u-grantee", &[]);
        assert!(
            s.get_fact(&grantee, "fact:a").await.unwrap().is_none(),
            "expired grant must not admit"
        );
        assert!(
            s.get_fact(&grantee, "fact:b").await.unwrap().is_some(),
            "active grant must admit"
        );

        s.revoke_grant_for_test("acme", "grant:live", now)
            .await
            .unwrap();
        assert!(
            s.get_fact(&grantee, "fact:b").await.unwrap().is_none(),
            "revocation is immediate by construction; no cache invalidation step exists"
        );
    }

    /// SA-GRANT-08, behavioural: a granted fact is fetchable by id and findable by no search leg.
    ///
    /// The counterpart to `vector_path_omits_the_grant_arm_under_both_profiles`, which asserts only on
    /// the filter string and would keep passing if the exclusion were inert. This pins the *effect*,
    /// and it pins it on purpose: RISK-020 records that the id/search asymmetry is a provisional
    /// product decision rather than a settled design. If someone later puts the grant arm on the
    /// search legs, this test fails and tells them they have changed a documented decision — which is
    /// the outcome wanted, rather than a silent behaviour change nobody reviewed.
    ///
    /// The exclusion reaches all three stage-1 signals, not only the vector one, because `recall`
    /// deliberately gives them one read filter so relevance cannot depend on which leg matched
    /// (RISK-017). That is why the assertion below is over `recall()` as a whole.
    #[tokio::test]
    async fn a_granted_fact_is_fetchable_by_id_and_found_by_no_search_leg() {
        let s = store().await;
        let owner = scope_ref("acme", None, "u-owner");
        s.put_fact(&sample_fact(
            "fact:shared",
            owner.clone(),
            Visibility::UserPrivate,
        ))
        .await
        .unwrap();

        let own = ctx("acme", "u-owner", &[]);
        s.set_fact_embedding(&own, "fact:shared", &[0.1_f32; DIM as usize], "m1")
            .await
            .unwrap();

        let now = Utc::now();
        s.put_grant_for_test(
            "acme",
            "grant:g-search",
            "fact:shared",
            &owner,
            "u-grantee",
            now - chrono::Duration::hours(1),
            now + chrono::Duration::hours(1),
        )
        .await
        .unwrap();

        let grantee = ctx("acme", "u-grantee", &[]);

        // Fetchable by id — the grant arm is live on `fact_read_filter`.
        assert!(
            s.get_fact(&grantee, "fact:shared").await.unwrap().is_some(),
            "the grant must admit a direct fetch"
        );

        // The owner finds it by search, so the query itself is sound.
        let q = StageOneQuery {
            query_vector: vec![0.1_f32; DIM as usize],
            keyword_terms: vec!["orders".into()],
            filters: RecallFilters::default(),
            scope: own.clone(),
            stage1_k: 50,
        };
        assert_eq!(
            s.recall(&own, &q).await.unwrap().len(),
            1,
            "the owner must find their own fact, or this test proves nothing about the grantee"
        );

        // The grantee does not, on any leg — vector, keyword or graph.
        let q_grantee = StageOneQuery {
            scope: grantee.clone(),
            ..q.clone()
        };
        assert!(
            s.recall(&grantee, &q_grantee).await.unwrap().is_empty(),
            "RISK-020: a granted fact is not findable by search. If this fails, the id/search \
             asymmetry has been changed — check that the change was a decision, not an accident"
        );
    }

    /// SA-GRANT-08: the vector path must not consult the grant arm.
    ///
    /// Both strategies are wrong for a grant-scoped read — `PostFilteredAnn` starves at a low visible
    /// fraction, and `FilterFirstExact`'s single-column index hint is unsound under a sub-select. A
    /// grant arm reaching either one would produce a search that silently under-returns, which is the
    /// worst failure available here: it looks like an answer.
    #[test]
    fn vector_path_omits_the_grant_arm_under_both_profiles() {
        for profile in [MemoryProfile::Collective, MemoryProfile::Individual] {
            let (read, _hint) = vector_read_filter(profile);
            assert!(
                !read.contains("grant"),
                "{profile:?} vector filter must not consult grants: {read}"
            );
        }
        // ...while the non-vector path does, so the two genuinely differ.
        assert!(fact_filter_clause().contains("grant"));
    }

    /// The SQL arm and `Grant::is_active_at` express one rule; they must not drift apart.
    #[test]
    fn grant_active_predicate_matches_its_sql_arm() {
        use crate::types::grant::{Grant, GrantOperation};
        use chrono::{Duration, Utc};

        let now = Utc::now();
        let mk = |from, to, revoked| Grant {
            id: "grant:test".into(),
            fact: "fact:test".into(),
            grantor: scope_ref("t", None, "owner"),
            grantee: "grantee".into(),
            purpose: "p".into(),
            operation: GrantOperation::Read,
            valid_from: from,
            valid_to: to,
            revoked_at: revoked,
            evidence: "e".into(),
            created_at: now,
        };
        let hour = Duration::hours(1);

        assert!(
            mk(now - hour, now + hour, None).is_active_at(now),
            "in window"
        );
        assert!(
            !mk(now + hour, now + hour * 2, None).is_active_at(now),
            "not yet valid"
        );
        assert!(
            !mk(now - hour * 2, now - hour, None).is_active_at(now),
            "expired"
        );
        assert!(
            !mk(now - hour, now + hour, Some(now)).is_active_at(now),
            "revoked"
        );
        // The SQL arm tests the same three columns, in the same order.
        let arm = grant_arm_sql();
        for column in ["grantee", "revoked_at", "valid_from", "valid_to"] {
            assert!(arm.contains(column), "{column} missing from {arm}");
        }
    }

    /// Stage 1 contract: the `individual` profile selects the filter-first strategy.
    ///
    /// The two forms are asserted to be mutually exclusive rather than merely different — filter-first
    /// emitting a KNN operator would mean it still walks the index and can still starve, which is the
    /// entire failure this strategy exists to remove.
    #[test]
    fn individual_profile_selects_filter_first_exact_scoring() {
        assert_eq!(
            VectorStrategy::for_profile(MemoryProfile::Individual),
            VectorStrategy::FilterFirstExact
        );
        let (read, hint) = vector_read_filter(MemoryProfile::Individual);
        let sql = vector_signal_sql(
            VectorStrategy::FilterFirstExact,
            50,
            read,
            hint,
            validity_clause(None),
            "",
        );
        assert!(
            sql.contains("vector::similarity::cosine(embedding, $qv)"),
            "filter-first must score exactly: {sql}"
        );
        assert!(
            !sql.contains("<|") && !sql.contains("vector::distance::knn()"),
            "filter-first must not walk the HNSW index — that is what lets it starve: {sql}"
        );
        assert!(
            sql.contains("embedding IS NOT NONE"),
            "unembedded facts are a normal steady state and must be excluded: {sql}"
        );
        // The cap is applied after the scope filter, not before it.
        assert!(
            sql.find("$cuser").unwrap() < sql.find("LIMIT $k").unwrap(),
            "the scope filter must precede the cap: {sql}"
        );
    }

    /// What actually separates the two strategies at a low visible fraction — measured, not assumed.
    ///
    /// **This test corrects an earlier claim.** RFC-03 Part 2 asserted that post-filtered ANN *starves*
    /// below a ~25% visible fraction: that the index returns its K nearest over the whole table and the
    /// scope predicate is AND-ed afterwards, so a caller who can read almost nothing gets nothing back.
    /// Against SurrealDB 3.1.5 that is false. `EXPLAIN` shows the predicate pushed **into** the index
    /// scan (`KnnScan { predicate: "owner.user = ..." }`), so the walk continues until it has K matching
    /// rows. Both strategies are correct at any visible fraction.
    ///
    /// The cost is latency, not lost results: continuing the walk until K *visible* nodes turn up
    /// degenerates towards a full graph traversal as the visible fraction falls. The size of that
    /// effect is measured by the `vector_strategy_crossover` benchmark below; this test pins the
    /// correctness half, which is the part that must hold on every run rather than only on a
    /// benchmark machine.
    #[tokio::test]
    async fn both_strategies_are_correct_at_a_low_visible_fraction() {
        let mut qv = vec![0.0_f32; DIM as usize];
        qv[0] = 1.0;
        let mut far = vec![0.0_f32; DIM as usize];
        far[DIM as usize - 1] = 1.0; // orthogonal to the query: the worst match present.

        // 40 facts private to one user, 1 private to another, and the lone visible one is deliberately
        // the *worst* scoring of the 41 — so nothing but the scope filter can put it in the result.
        async fn populate(s: &Store, qv: &[f32], far: &[f32]) {
            let noise_ctx = ctx("acme", "u-noise", &[]);
            for i in 0..40 {
                let id = format!("fact:noise{i}");
                let f = sample_fact(
                    &id,
                    scope_ref("acme", None, "u-noise"),
                    Visibility::UserPrivate,
                );
                s.put_fact(&f).await.unwrap();
                s.set_fact_embedding(&noise_ctx, &id, qv, "m1")
                    .await
                    .unwrap();
            }
            let solo_ctx = ctx("acme", "u-solo", &[]);
            let f = sample_fact(
                "fact:solo",
                scope_ref("acme", None, "u-solo"),
                Visibility::UserPrivate,
            );
            s.put_fact(&f).await.unwrap();
            s.set_fact_embedding(&solo_ctx, "fact:solo", far, "m1")
                .await
                .unwrap();
        }

        let solo = ctx("acme", "u-solo", &[]);
        let q = StageOneQuery {
            query_vector: qv.clone(),
            keyword_terms: vec![], // vector signal only — the other signals would mask the effect.
            filters: RecallFilters::default(),
            scope: solo.clone(),
            stage1_k: 5,
        };

        for profile in [MemoryProfile::Collective, MemoryProfile::Individual] {
            let s = Store::new_in_memory_with_profile(DIM, profile)
                .await
                .unwrap();
            populate(&s, &qv, &far).await;
            let got = s.recall(&solo, &q).await.unwrap();
            let ids: Vec<&str> = got.iter().map(|c| c.fact_id.as_str()).collect();
            assert_eq!(
                ids,
                vec!["fact:solo"],
                "{profile}: expected the one visible fact"
            );
            assert!(
                (0.0..=1.0).contains(&got[0].semantic_score),
                "{profile}: score out of range: {}",
                got[0].semantic_score
            );
        }
    }

    /// Measure where the two strategies cross over. **Not run by default** — it is a measurement, not
    /// an assertion, and its absolute numbers are machine-dependent.
    ///
    /// ```text
    /// cargo test --release --lib vector_strategy_crossover -- --ignored --nocapture
    /// ```
    ///
    /// The independent variable is the **visible fraction**: the namespace holds a fixed number of
    /// facts and the caller can read a varying share of them. Both strategies run against the same
    /// engine and the same rows, so the comparison isolates the strategy rather than the data.
    ///
    /// Only the ratio between the two columns is meaningful. Absolute latency here is not a check
    /// against NFR-P3: this is an in-process `mem://` engine with a synthetic corpus, not the
    /// SurrealKV-backed deployment NFR-P3 is written against. Measuring against NFR-P3 is the
    /// re-pointed benchmark harness's job.
    #[tokio::test]
    #[ignore = "measurement, not an assertion; run explicitly with --release"]
    async fn vector_strategy_crossover() {
        const N: usize = 2000;
        const REPS: u32 = 5;
        let dim: usize = std::env::var("MEMORY_BENCH_DIM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024);

        let mut qv = vec![0.0_f32; dim];
        qv[0] = 1.0;

        let s = Store::new_in_memory_with_profile(dim as u32, MemoryProfile::Collective)
            .await
            .unwrap();

        // One namespace of N tenant-private facts. `visible` of them belong to `u-me`; the rest belong
        // to `u-other` and are unreadable. Ownership decides visibility, so the visible fraction can be
        // varied without touching the corpus.
        let visible_counts = [1usize, 10, 100, 500, 1000, 2000];
        let max_visible = *visible_counts.iter().max().unwrap();
        let me = ctx("acme", "u-me", &[]);
        let other = ctx("acme", "u-other", &[]);
        for i in 0..N {
            // The first `max_visible` facts are re-owned per leg below; seed everything to u-other.
            let id = format!("fact:b{i}");
            let f = sample_fact(
                &id,
                scope_ref("acme", None, "u-other"),
                Visibility::UserPrivate,
            );
            s.put_fact(&f).await.unwrap();
            let mut v = qv.clone();
            v[1] = i as f32 * 0.00001; // distinct, all near the query.
            s.set_fact_embedding(&other, &id, &v, "m1").await.unwrap();
        }
        assert!(max_visible <= N);

        println!("\n  dim={dim}  namespace={N} facts  stage1_k=10  reps={REPS} (median ms)");
        println!(
            "  {:>8}  {:>9}  {:>12}  {:>12}  {:>12}  {:>8}",
            "visible", "fraction", "post-ANN", "f1st-plain", "f1st-hinted", "ANN/hinted"
        );

        for &visible in &visible_counts {
            // Re-own exactly `visible` facts to u-me, and the remainder back to u-other.
            for i in 0..N {
                let owner = if i < visible { "u-me" } else { "u-other" };
                s.db.query(
                    s.scoped(
                        "acme",
                        format!("UPDATE fact:b{i} SET owner.user = '{owner}'"),
                    )
                    .unwrap(),
                )
                .await
                .unwrap()
                .check()
                .unwrap();
            }

            let q = StageOneQuery {
                query_vector: qv.clone(),
                keyword_terms: vec![],
                filters: RecallFilters::default(),
                scope: me.clone(),
                stage1_k: 10,
            };

            let mut timings = Vec::new();
            let legs = [
                (VectorStrategy::PostFilteredAnn, MemoryProfile::Collective),
                (VectorStrategy::FilterFirstExact, MemoryProfile::Collective),
                (VectorStrategy::FilterFirstExact, MemoryProfile::Individual),
            ];
            for (strategy, leg_profile) in legs {
                let (read, hint) = vector_read_filter(leg_profile);
                let sql = vector_signal_sql(strategy, 10, read, hint, validity_clause(None), "");
                let mut runs = Vec::new();
                for rep in 0..=REPS {
                    let t = std::time::Instant::now();
                    let mut resp = s
                        .bind_recall(s.db.query(s.scoped("acme", &sql).unwrap()), &me, &q)
                        .bind(("qv", qv.clone()))
                        .bind(("k", 10i64))
                        .await
                        .unwrap();
                    let rows: Vec<Json> = resp.take(1).unwrap();
                    if rep > 0 {
                        // Discard rep 0 as warm-up.
                        runs.push(t.elapsed().as_secs_f64() * 1000.0);
                    }
                    assert_eq!(
                        rows.len(),
                        visible.min(10),
                        "{strategy:?} at visible={visible} returned the wrong row count — a strategy \
                         that returns fewer rows is not merely slower, and timing it would be \
                         comparing different work"
                    );
                }
                runs.sort_by(|a, b| a.partial_cmp(b).unwrap());
                timings.push(runs[runs.len() / 2]);
            }

            println!(
                "  {:>8}  {:>8.1}%  {:>12.2}  {:>12.2}  {:>12.2}  {:>7.1}x",
                visible,
                (visible as f64 / N as f64) * 100.0,
                timings[0],
                timings[1],
                timings[2],
                timings[0] / timings[2]
            );
        }
        println!();
    }

    /// RISK-017: a co-resident tenant's facts must be invisible, including tenant-shared ones.
    ///
    /// The read-filter's `OR visibility = 'tenant-shared'` arm carried no tenant predicate, and the
    /// other two arms matched on user and team ids that are only unique *within* a tenant. All three
    /// were safe solely because ADR-011 puts one tenant in one namespace — an invariant RFC-03
    /// Stage 2 removes deliberately.
    ///
    /// No public API can produce this state today, because `put_fact` scopes by the fact's own
    /// `owner.tenant`. So the test writes a foreign tenant's fact directly into the namespace, which is
    /// exactly the row layout Stage 2 creates on purpose. Against the previous filter the
    /// tenant-shared fact was returned to an unrelated tenant's caller; this asserts it is not.
    ///
    /// Worth having now rather than at Stage 2: it is a cross-tenant disclosure, the fix is inert
    /// today (every fact in a namespace already carries that namespace's tenant, so the new conjunct
    /// changes no result), and a security fix that cannot be exercised is a security fix nobody can
    /// trust.
    /// One valid row in every table of `ERASURE_SET`, owned by `tenant`, written into `namespace`.
    ///
    /// Written as SurrealQL literals rather than through the C1 write API: most of these tables are
    /// SCHEMAFULL and owned by other components, so there is no C1 method to reach them, and the
    /// erasure set has to be proved against the real schema rather than a convenient subset of it.
    /// Every row carries a distinct idempotency key because two of the tables index that field
    /// UNIQUE.
    async fn seed_every_table(s: &Store, namespace: &str, tenant: &str, key: &str) {
        let rows: [&str; 11] = [
            "CREATE audit_log CONTENT { subject: 'u', operation: 'recall', \
             scope: { tenant: $t, user: 'u' }, outcome: 'ok', token_jti: 'j', \
             correlation_id: 'c', at: time::now() }",
            "CREATE dead_letter CONTENT { kind: 'extract_fact', payload: {}, \
             scope: { tenant: $t, user: 'u' }, idempotency_key: $k, attempts: 5, \
             original_job_id: 'work_job:x', created_at: time::now(), dead_lettered_at: time::now() }",
            "CREATE entity CONTENT { canonical_name: 'Orders', aliases: [], \
             owner: { tenant: $t, user: 'u' } }",
            "CREATE fact CONTENT { content: {}, content_text: 'x', entities: ['entity:e1'], \
             memory_class: 'semantic', visibility: 'tenant-shared', owner: { tenant: $t, user: 'u' }, \
             valid_from: time::now(), ingested_at: time::now(), confidence: 0.9, salience: 0.5, \
             stability: 1.0 }",
            "CREATE grant CONTENT { fact: 'fact:f1', grantor: { tenant: $t, user: 'u' }, \
             grantee: 'u-other', purpose: 'care', operation: 'read', valid_from: time::now(), \
             valid_to: time::now() + 1d, evidence: 'e', created_at: time::now() }",
            "CREATE idempotency_record CONTENT { tenant: $t, user: 'u', route: 'POST /v1/memories', \
             idempotency_key: $k, response_body: {}, status_code: 200, expires_at: time::now() + 1d }",
            "CREATE maintenance_state CONTENT { tenant: $t }",
            "CREATE quarantine CONTENT { content: {}, entities: ['entity:e1'], \
             memory_class: 'semantic', owner: { tenant: $t, user: 'u' }, trust_score: 0.4, \
             confidence: 0.5, salience: 0.5, is_instruction_like: false, idempotency_key: $k, \
             reason: 'low trust', quarantined_at: time::now() }",
            "CREATE relationship CONTENT { kind: 'owns', from_ent: 'entity:e1', to_ent: 'entity:e2', \
             valid_from: time::now(), confidence: 0.8, owner: { tenant: $t, user: 'u' } }",
            "CREATE source CONTENT { origin_ref: 'doc://x', trust_signal: 0.7, \
             owner: { tenant: $t, user: 'u' } }",
            "CREATE work_job CONTENT { kind: 'extract_fact', payload: {}, \
             scope: { tenant: $t, user: 'u' }, idempotency_key: $k, attempts: 0, status: 'pending', \
             not_before: time::now(), created_at: time::now() }",
        ];
        for sql in rows {
            s.db.query(s.scoped(namespace, sql).unwrap())
                .bind(("t", tenant.to_string()))
                .bind(("k", key.to_string()))
                .await
                .unwrap_or_else(|e| panic!("seed failed for {sql}: {e}"))
                .check()
                .unwrap_or_else(|e| panic!("seed rejected for {sql}: {e}"));
        }
    }

    /// The RISK-021 regression guard. Erasure must remove every row carrying the erased tenant and
    /// nothing else, in a namespace that holds a second tenant — the row layout the topology stage
    /// creates on purpose and that no public API can produce today, which is exactly why the suite
    /// would otherwise never see it.
    #[tokio::test]
    async fn erase_tenant_removes_every_table_and_leaves_a_co_resident_untouched() {
        let s = store().await;
        s.ensure_provisioned("acme").await.unwrap();
        seed_every_table(&s, "acme", "acme", "k-acme").await;
        seed_every_table(&s, "acme", "globex", "k-globex").await;

        for (table, field) in ERASURE_SET {
            assert_eq!(
                s.count_table_rows("acme", table, field).await.unwrap(),
                1,
                "{table}: acme must have one row before the erasure"
            );
        }

        let proof = s.erase_tenant("acme").await.unwrap();

        assert_eq!(proof.tenant, "acme");
        assert_eq!(proof.namespace, "acme");
        assert_eq!(proof.removed.len(), ERASURE_SET.len());
        assert_eq!(proof.total_removed, ERASURE_SET.len() as u64);
        assert_eq!(
            proof.removed.values().sum::<u64>(),
            proof.total_removed,
            "the per-table counts must sum to the total the proof reports"
        );
        assert_eq!(
            proof.digest,
            sha256_hex(&erasure_digest_input("acme", "acme", &proof.removed)),
            "the digest must be taken over the counts the proof carries"
        );

        // Nothing of acme survives, and everything of the co-resident does. The second half is the
        // point of the test: `REMOVE NAMESPACE` would have passed the first half and failed this one.
        for (table, field) in ERASURE_SET {
            let mut resp =
                s.db.query(
                    s.scoped(
                        "acme",
                        format!("SELECT count() FROM {table} WHERE {field} = 'globex' GROUP ALL"),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            let rows: Vec<Json> = resp.take(1).unwrap();
            let survivors = rows
                .first()
                .and_then(|r| r.get("count"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            assert_eq!(
                s.count_table_rows("acme", table, field).await.unwrap(),
                0,
                "{table}: no erased-tenant row may remain"
            );
            assert_eq!(survivors, 1, "{table}: the co-resident's row must survive");
        }
    }

    /// Erasing a tenant that was never written is a zero proof, not an error — a data subject with no
    /// data still gets an answer, and the digest still covers every table so the two proofs are
    /// comparable.
    #[tokio::test]
    async fn erasing_an_unwritten_tenant_returns_a_zero_proof() {
        let s = store().await;
        let proof = s.erase_tenant("newco").await.unwrap();
        assert_eq!(proof.total_removed, 0);
        assert_eq!(proof.removed.len(), ERASURE_SET.len());
        assert!(proof.removed.values().all(|c| *c == 0));
        assert!(!proof.digest.is_empty());
    }

    /// The erasure set must stay exhaustive: a table added to the schema without an entry leaves the
    /// erased tenant's rows behind while the proof reports success. `schema_migrations` is excluded
    /// deliberately — it is the namespace's schema version, not any tenant's data.
    #[tokio::test]
    async fn the_erasure_set_covers_every_table_the_migrations_define() {
        let s = store().await;
        s.ensure_provisioned("acme").await.unwrap();
        let mut resp =
            s.db.query(s.scoped("acme", "INFO FOR DB").unwrap())
                .await
                .unwrap();
        let info: Vec<Json> = resp.take(1).unwrap();
        let tables: Vec<String> = info
            .first()
            .and_then(|v| v.get("tables"))
            .and_then(|v| v.as_object())
            .map(|o| o.keys().cloned().collect())
            .expect("INFO FOR DB reports a tables object");
        assert!(!tables.is_empty(), "the namespace must define tables");

        let covered: std::collections::HashSet<&str> =
            ERASURE_SET.iter().map(|(t, _)| *t).collect();
        for t in &tables {
            if t == "schema_migrations" {
                continue;
            }
            assert!(
                covered.contains(t.as_str()),
                "table `{t}` carries tenant data but is not in ERASURE_SET"
            );
        }
    }

    /// The namespace drop refuses where the namespace is not the tenant's own. Driven through the
    /// guard directly: placement is identity under every profile that exists, so the store cannot be
    /// constructed into this state, and the branch would otherwise be asserted by nothing.
    #[test]
    fn the_namespace_drop_is_refused_when_the_namespace_is_not_exclusive() {
        let pooled = Placement {
            namespace: "pool_0".into(),
            database: DB_NAME.into(),
        };
        for profile in [MemoryProfile::Collective, MemoryProfile::Individual] {
            match ensure_namespace_exclusive(profile, "acme", &pooled) {
                Err(StoreError::NamespaceNotExclusive { tenant, namespace }) => {
                    assert_eq!(tenant, "acme");
                    assert_eq!(namespace, "pool_0");
                }
                other => panic!("{profile}: expected a refusal, got {other:?}"),
            }
            // The exclusive case still passes, so the guard is not simply refusing everything.
            let own = Placement {
                namespace: "acme".into(),
                database: DB_NAME.into(),
            };
            ensure_namespace_exclusive(profile, "acme", &own).expect("own namespace is droppable");
        }
    }

    #[tokio::test]
    async fn a_co_resident_tenants_facts_are_invisible() {
        let s = store().await;
        s.ensure_provisioned("acme").await.unwrap();

        // One acme fact the caller legitimately owns, so a passing test cannot be a filter that
        // rejects everything.
        let mine = sample_fact(
            "fact:mine",
            scope_ref("acme", None, "u-sarah"),
            Visibility::TenantShared,
        );
        s.put_fact(&mine).await.unwrap();

        // Three globex facts written into acme's namespace, one per arm of the read-filter. The user
        // and team ids collide with acme's on purpose: that collision is what the tenant conjunct has
        // to defeat, and it is realistic — ids are unique within a tenant, not across tenants.
        for (id, vis, user, team) in [
            (
                "fact:foreign_shared",
                Visibility::TenantShared,
                "u-other",
                None,
            ),
            (
                "fact:foreign_team",
                Visibility::TeamShared,
                "u-other",
                Some("alpha"),
            ),
            (
                "fact:foreign_user",
                Visibility::UserPrivate,
                "u-sarah",
                None,
            ),
        ] {
            let f = sample_fact(id, scope_ref("globex", team, user), vis);
            let obj = convert::fact_to_object(&f);
            s.db.query(s.scoped("acme", "UPSERT $id CONTENT $rec").unwrap())
                .bind(("id", id_thing(id).unwrap()))
                .bind(("rec", Value::Object(obj)))
                .await
                .unwrap()
                .check()
                .unwrap();
        }

        let c = ctx("acme", "u-sarah", &["alpha"]);
        s.set_fact_embedding(&c, "fact:mine", &[0.1_f32; DIM as usize], "m1")
            .await
            .unwrap();

        // Point reads: every foreign fact is invisible, including the one whose owner.user matches.
        assert!(
            s.get_fact(&c, "fact:mine").await.unwrap().is_some(),
            "own fact must be readable"
        );
        for id in [
            "fact:foreign_shared",
            "fact:foreign_team",
            "fact:foreign_user",
        ] {
            assert!(
                s.get_fact(&c, id).await.unwrap().is_none(),
                "{id}: a co-resident tenant's fact must not be readable"
            );
        }

        // Retrieval: all three signals share the read-filter, so all three are checked at once.
        let q = StageOneQuery {
            query_vector: vec![0.1_f32; DIM as usize],
            keyword_terms: vec!["orders".into()],
            filters: RecallFilters {
                entity: Some("entity:e1".into()),
                ..Default::default()
            },
            scope: c.clone(),
            stage1_k: 50,
        };
        let ids: Vec<String> = s
            .recall(&c, &q)
            .await
            .unwrap()
            .into_iter()
            .map(|x| x.fact_id)
            .collect();
        assert_eq!(
            ids,
            vec!["fact:mine".to_string()],
            "recall leaked a co-resident tenant's facts"
        );

        // The maintenance scope bypasses user and visibility, but must not bypass the tenant — it
        // mutates what it reads, so a leak here is a cross-tenant write rather than a disclosure.
        let maint = ScopeContext {
            tenant: "acme".into(),
            teams: vec![],
            user: String::new(),
            token_jti: "maintenance".into(),
            allowed_ops: OpSet {
                read: true,
                write: true,
                forget: true,
            },
            client: None,
            fact_authority: None,
            correlation_id: "c-maint".into(),
        };
        assert!(
            s.get_fact(&maint, "fact:mine").await.unwrap().is_some(),
            "maintenance reads its own tenant"
        );
        assert!(
            s.get_fact(&maint, "fact:foreign_shared")
                .await
                .unwrap()
                .is_none(),
            "maintenance must not reach a co-resident tenant's facts"
        );
    }

    /// RISK-018 regression guard: the graph leg must be served by `fact_ent`.
    ///
    /// Two things have to hold together and neither is visible in the query text. Migration 0002 has to
    /// define `fact_ent` over `entities.*` — over the whole array (0001's form) the index cannot serve
    /// element matching, and hinting it degrades to a full table scan rather than falling back. And the
    /// `WITH INDEX` hint has to be present — with a correct index but no hint the planner still prefers
    /// `fact_valid`, which matches every live fact. Drop either and this silently becomes a table scan
    /// that returns the right answer, so only the plan can catch it.
    #[tokio::test]
    async fn graph_leg_is_served_by_the_entity_index() {
        let s = seeded_store_for_index_checks().await;
        let sql = format!(
            "SELECT meta::id(id) AS fid FROM fact WITH INDEX fact_ent \
             WHERE $entity IN entities AND ({}) AND {} LIMIT $k",
            fact_filter_clause(),
            validity_clause(None)
        );
        assert_eq!(
            explain_index_for(&s, &sql).await.as_deref(),
            Some("fact_ent"),
            "graph leg must use fact_ent; `None` means the hint forced a table scan (0002 missing), \
             `fact_valid` means the hint was dropped"
        );

        // The plan is only worth pinning if the rows are right: every seeded fact carries entity:e1.
        let c = ctx("acme", "u-sarah", &[]);
        let q = StageOneQuery {
            query_vector: vec![],
            keyword_terms: vec![],
            filters: RecallFilters {
                entity: Some("entity:e1".into()),
                ..Default::default()
            },
            scope: c.clone(),
            stage1_k: 50,
        };
        let got = s.recall(&c, &q).await.unwrap();
        assert_eq!(
            got.len(),
            40,
            "the entity filter must still return every matching fact"
        );
    }

    /// FU-025 regression guard: `hard_delete`'s lineage query must be index-served.
    ///
    /// Unlike the graph leg this one needs no `WITH INDEX` hint — it carries no bi-temporal predicate,
    /// so there is no `fact_valid` for the planner to prefer, and an unnecessary hint would be one
    /// more thing to keep sound. That makes the plan the only thing holding it in place, and the plan
    /// is exactly what a future edit would change without changing a single returned row: adding a
    /// validity predicate here would flip the choice the way it did on the graph leg.
    #[tokio::test]
    async fn lineage_query_is_index_served() {
        let s = seeded_store_for_index_checks().await;
        let sql = format!(
            "SELECT * FROM fact WHERE $base IN derived_from AND ({})",
            fact_filter_clause()
        );
        assert_eq!(
            explain_index_for(&s, &sql).await.as_deref(),
            Some("fact_derived"),
            "lineage query must use fact_derived; `None` means it is scanning the fact table again"
        );
    }

    /// `list_tenants` is root-level and must read result slot 0, not 1.
    ///
    /// The RISK-012 fix shifted every *scoped* statement's `take(n)` to `take(n+1)`, because a scoped
    /// statement's batch begins with the `USE`. This method is deliberately unscoped — `INFO FOR ROOT`
    /// enumerates namespaces and belongs to no tenant — and was caught by that shift, which made it
    /// fail outright. Nothing in the suite covered it, and both queue consumers depend on it to find
    /// the tenants they serve, so the gap is worth a test rather than only a corrected index.
    #[tokio::test]
    async fn list_tenants_enumerates_provisioned_namespaces() {
        let s = store().await;
        for t in ["acme", "globex"] {
            s.ensure_provisioned(t).await.unwrap();
        }
        let mut got = s.list_tenants().await.expect("list_tenants must not error");
        got.sort();
        assert!(
            got.contains(&"acme".to_string()) && got.contains(&"globex".to_string()),
            "expected both provisioned namespaces, got {got:?}"
        );
    }

    /// RISK-018 diagnostic: report which index the planner picks for every scope-filtered read.
    ///
    /// ```text
    /// cargo test --lib index_choice_report -- --ignored --nocapture
    /// ```
    ///
    /// SurrealDB 3.1.5 has no selectivity statistics, so index choice is not something that can be
    /// reasoned about from the query text — it has to be read off `EXPLAIN`. This prints the choice for
    /// each statement C1 issues against `fact`, so a new query or an engine upgrade can be checked
    /// rather than assumed. `graph_leg_uses_the_entity_index` asserts the one case that was wrong.
    #[tokio::test]
    #[ignore = "diagnostic, not an assertion; run explicitly"]
    async fn index_choice_report() {
        let s = seeded_store_for_index_checks().await;
        let cases: Vec<(&str, String)> = vec![
            ("vector / filter-first", {
                let (read, hint) = vector_read_filter(MemoryProfile::Individual);
                vector_signal_sql(
                    VectorStrategy::FilterFirstExact,
                    10,
                    read,
                    hint,
                    validity_clause(None),
                    "",
                )
            }),
            (
                "keyword (BM25)",
                format!(
                    "SELECT meta::id(id) AS fid, search::score(0) AS s FROM fact \
                 WHERE content_text @0@ $terms AND ({}) AND {} ORDER BY s DESC LIMIT $k",
                    fact_filter_clause(),
                    validity_clause(None)
                ),
            ),
            (
                "graph (entity)",
                format!(
                    "SELECT meta::id(id) AS fid FROM fact WITH INDEX fact_ent \
                 WHERE $entity IN entities AND ({}) AND {} LIMIT $k",
                    fact_filter_clause(),
                    validity_clause(None)
                ),
            ),
            (
                "graph, unhinted",
                format!(
                    "SELECT meta::id(id) AS fid FROM fact \
                 WHERE $entity IN entities AND ({}) AND {} LIMIT $k",
                    fact_filter_clause(),
                    validity_clause(None)
                ),
            ),
            (
                "lineage (derived_from)",
                format!(
                    "SELECT * FROM fact WHERE $base IN derived_from AND ({})",
                    fact_filter_clause()
                ),
            ),
            (
                "C7 decay scan",
                "SELECT * FROM fact WHERE valid_to IS NONE AND salience < $floor LIMIT $limit"
                    .into(),
            ),
        ];
        for (label, sql) in cases {
            println!("  {label:24} -> {:?}", explain_index_for(&s, &sql).await);
        }
    }

    /// Seed one namespace with enough shape for the planner to have a choice to make.
    async fn seeded_store_for_index_checks() -> Store {
        let s = store().await;
        let c = ctx("acme", "u-sarah", &[]);
        for i in 0..40 {
            let id = format!("fact:ix{i}");
            let f = sample_fact(
                &id,
                scope_ref("acme", None, "u-sarah"),
                Visibility::TenantShared,
            );
            s.put_fact(&f).await.unwrap();
            s.set_fact_embedding(&c, &id, &[0.1_f32; DIM as usize], "m1")
                .await
                .unwrap();
        }
        s
    }

    /// The index name the planner picks for `sql`, or `None` if it plans a full table scan.
    ///
    /// Reads the `index` attribute out of the `EXPLAIN` plan. Every parameter the statements use is
    /// bound unconditionally — an unused binding is harmless, and binding them individually per case
    /// would make the report harder to extend than to read.
    async fn explain_index_for(s: &Store, sql: &str) -> Option<String> {
        let mut resp =
            s.db.query(s.scoped("acme", format!("{sql} EXPLAIN")).unwrap())
                .bind(("qv", vec![0.1_f32; DIM as usize]))
                .bind(("terms", "orders".to_string()))
                .bind(("entity", "entity:e1".to_string()))
                .bind(("base", "fact:ix0".to_string()))
                .bind(("ctenant", "acme".to_string()))
                .bind(("cuser", "u-sarah".to_string()))
                .bind(("cteams", Vec::<String>::new()))
                .bind(("cnow", Utc::now()))
                .bind(("k", 10i64))
                .bind(("floor", 0.1f64))
                .bind(("limit", 10i64))
                .await
                .unwrap();
        let rows: Vec<Json> = resp.take(1).unwrap();
        let plan = serde_json::to_string(&rows).unwrap();
        plan.split("\"index\":\"")
            .nth(1)
            .map(|x| x.split('"').next().unwrap().to_string())
    }

    /// The measurement that actually decides the consumer profile: hold one user's corpus fixed and
    /// grow the number of co-resident users around them. **Not run by default.**
    ///
    /// ```text
    /// cargo test --release --lib co_residency_cost -- --ignored --nocapture
    /// ```
    ///
    /// `vector_strategy_crossover` varies the visible *fraction* at a fixed namespace size, which
    /// conflates two things. This one separates them: the caller always sees 1,000 facts — one
    /// consumer's memory — while the namespace grows underneath them. That is exactly the Stage 2
    /// question, because filter-first's cost should track the visible set while the index walk's
    /// tracks the whole namespace.
    #[tokio::test]
    #[ignore = "measurement, not an assertion; run explicitly with --release"]
    async fn co_residency_cost() {
        const VISIBLE: usize = 1000;
        const REPS: u32 = 3;
        let dim: usize = std::env::var("MEMORY_BENCH_DIM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024);
        let sizes: Vec<usize> = std::env::var("MEMORY_BENCH_SIZES")
            .unwrap_or_else(|_| "2000,8000,20000".into())
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();

        let mut qv = vec![0.0_f32; dim];
        qv[0] = 1.0;
        let me = ctx("acme", "u-me", &[]);

        println!("\n  dim={dim}  visible={VISIBLE} (one consumer's memory)  stage1_k=10  reps={REPS} (median ms)");
        println!(
            "  {:>10}  {:>9}  {:>12}  {:>12}  {:>10}",
            "namespace", "fraction", "post-ANN", "f1st-hinted", "winner"
        );

        for &n in &sizes {
            let s = Store::new_in_memory_with_profile(dim as u32, MemoryProfile::Collective)
                .await
                .unwrap();
            let other = ctx("acme", "u-other", &[]);
            for i in 0..n {
                let id = format!("fact:c{i}");
                let owner = if i < VISIBLE { "u-me" } else { "u-other" };
                let f = sample_fact(&id, scope_ref("acme", None, owner), Visibility::UserPrivate);
                s.put_fact(&f).await.unwrap();
                let mut v = qv.clone();
                v[1] = i as f32 * 0.000001;
                s.set_fact_embedding(&other, &id, &v, "m1").await.unwrap();
            }

            let q = StageOneQuery {
                query_vector: qv.clone(),
                keyword_terms: vec![],
                filters: RecallFilters::default(),
                scope: me.clone(),
                stage1_k: 10,
            };

            let mut timings = Vec::new();
            for (strategy, leg_profile) in [
                (VectorStrategy::PostFilteredAnn, MemoryProfile::Collective),
                (VectorStrategy::FilterFirstExact, MemoryProfile::Individual),
            ] {
                let (read, hint) = vector_read_filter(leg_profile);
                let sql = vector_signal_sql(strategy, 10, read, hint, validity_clause(None), "");
                let mut runs = Vec::new();
                for rep in 0..=REPS {
                    let t = std::time::Instant::now();
                    let mut resp = s
                        .bind_recall(s.db.query(s.scoped("acme", &sql).unwrap()), &me, &q)
                        .bind(("qv", qv.clone()))
                        .bind(("k", 10i64))
                        .await
                        .unwrap();
                    let rows: Vec<Json> = resp.take(1).unwrap();
                    if rep > 0 {
                        runs.push(t.elapsed().as_secs_f64() * 1000.0);
                    }
                    assert_eq!(
                        rows.len(),
                        10,
                        "{strategy:?} at n={n} returned {} rows",
                        rows.len()
                    );
                }
                runs.sort_by(|a, b| a.partial_cmp(b).unwrap());
                timings.push(runs[runs.len() / 2]);
            }
            println!(
                "  {:>10}  {:>8.1}%  {:>12.2}  {:>12.2}  {:>10}",
                n,
                (VISIBLE as f64 / n as f64) * 100.0,
                timings[0],
                timings[1],
                if timings[1] < timings[0] {
                    "filter-1st"
                } else {
                    "post-ANN"
                }
            );
        }
        println!();
    }

    /// The relevance gate: where the visible fraction is high, the two strategies must agree.
    ///
    /// Equality of the *ordered* id list, not just the set, is the stronger claim and the one that
    /// matters — a strategy that returns the same facts in a different order changes what survives
    /// stage-2 re-ranking. The ANN arm is approximate in general; at this size it is exact, which is
    /// what makes the comparison meaningful rather than flaky.
    #[tokio::test]
    async fn both_strategies_agree_when_the_visible_fraction_is_high() {
        // Ten tenant-shared facts, each at a distinguishable distance from the query.
        async fn populate(s: &Store) {
            let c = ctx("acme", "u-sarah", &[]);
            for i in 0..10u32 {
                let id = format!("fact:v{i}");
                let f = sample_fact(
                    &id,
                    scope_ref("acme", None, "u-sarah"),
                    Visibility::TenantShared,
                );
                s.put_fact(&f).await.unwrap();
                let mut v = vec![0.0_f32; DIM as usize];
                v[0] = 1.0;
                v[1] = i as f32 * 0.1; // monotonically increasing angle from the query.
                s.set_fact_embedding(&c, &id, &v, "m1").await.unwrap();
            }
        }

        let mut qv = vec![0.0_f32; DIM as usize];
        qv[0] = 1.0;
        let c = ctx("acme", "u-sarah", &[]);
        let q = StageOneQuery {
            query_vector: qv,
            keyword_terms: vec![],
            filters: RecallFilters::default(),
            scope: c.clone(),
            stage1_k: 10,
        };

        let collective = Store::new_in_memory_with_profile(DIM, MemoryProfile::Collective)
            .await
            .unwrap();
        populate(&collective).await;
        let a = collective.recall(&c, &q).await.unwrap();

        let individual = Store::new_in_memory_with_profile(DIM, MemoryProfile::Individual)
            .await
            .unwrap();
        populate(&individual).await;
        let b = individual.recall(&c, &q).await.unwrap();

        let ids = |v: &[Candidate]| v.iter().map(|c| c.fact_id.clone()).collect::<Vec<_>>();
        assert_eq!(a.len(), 10, "all ten facts are visible to this caller");
        assert_eq!(
            ids(&a),
            ids(&b),
            "at a high visible fraction the strategies must agree on both membership and order"
        );

        // And the scores agree, not merely the ordering — filter-first computes the same cosine the
        // index reports, so a difference here would mean one of the two is scoring wrongly.
        for (x, y) in a.iter().zip(b.iter()) {
            assert!(
                (x.semantic_score - y.semantic_score).abs() < 1e-6,
                "{}: {} vs {}",
                x.fact_id,
                x.semantic_score,
                y.semantic_score
            );
        }
    }

    // ================================================================================================
    // RISK-012 — FIXED January 2026. Retained as the regression guard.
    //
    // The defect, as it was: `Store` held ONE `Surreal<Any>` connection shared as `Arc<Store>`, tenant
    // selection mutated that shared session (`use_ns`/`use_db`), and every store method was
    // select-then-query across two await points — so the namespace a statement ran against was decided
    // by task scheduling, not by the caller's `ScopeContext`. Measured: 70-75 of 100 concurrent writes
    // landed in the wrong tenant's namespace.
    //
    // The fix: every statement now carries its own `USE NS <t> DB memory;` in the same batch
    // (`Migrator::scoped`), and `ensure_provisioned` provisions without selecting. A batch is one
    // request, so no concurrent task can switch the namespace mid-operation. The tests below assert
    // the correct behaviour and now pass; they fail again if session-based selection returns.
    //
    // Historical note on the original shape:
    // `Store` holds ONE `Surreal<Any>` connection, shared as `Arc<Store>` and cloned to C2's queue and
    // reaper via `handle()`. Tenant selection mutates that shared session (`use_ns`/`use_db` in
    // `use_tenant`), and every store method is shaped as two separate await points:
    //
    //     self.ensure_provisioned(&ctx.tenant).await?;   // <- sets the session namespace
    //     self.db.query(...).await                   // <- runs in whatever namespace is current NOW
    //
    // There is no Mutex, RwLock, Semaphore or per-tenant pool anywhere in this module. So the namespace
    // a statement executes against is decided by task scheduling, not by the caller's `ScopeContext`.
    //
    // The existing `cross_tenant_isolation_on_recall` test above passes because it is entirely
    // SEQUENTIAL — one task, one namespace at a time. That is why this was never caught.
    //
    // Both tests below assert the CORRECT behaviour (isolation holds) and therefore FAIL while the
    // defect is present. Their failure is the confirmation; once store access is made session-free
    // they pass and stand as the regression guard.
    // ================================================================================================

    /// Deterministic guard against the return of session-based namespace selection.
    ///
    /// Originally this reproduced the defect by forcing the interleaving directly: task A called the
    /// old `ensure_and_use("acme")`, task B then called it for `globex` on the same shared session, and
    /// A's subsequent read returned globex's fact. That method no longer exists — selection now travels
    /// with each statement via `Migrator::scoped` — so the test is rewritten to guard the behaviour
    /// rather than the vanished mechanism.
    ///
    /// Two barriers still force maximum overlap: both tasks sit at the same point and are released
    /// together, so their reads execute concurrently on the one shared connection. If anyone
    /// reintroduces a `use_ns`-then-query shape, this fails again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_reads_do_not_see_another_tenants_facts() {
        use std::sync::Arc;
        use tokio::sync::Barrier;

        let s = Arc::new(store().await);

        s.put_fact(&sample_fact(
            "fact:acme_only",
            scope_ref("acme", None, "u-a"),
            Visibility::TenantShared,
        ))
        .await
        .unwrap();
        s.put_fact(&sample_fact(
            "fact:globex_only",
            scope_ref("globex", None, "u-b"),
            Visibility::TenantShared,
        ))
        .await
        .unwrap();

        let gate = Arc::new(Barrier::new(2));

        let task =
            |tenant: &'static str, user: &'static str, own: &'static str, foreign: &'static str| {
                let s = Arc::clone(&s);
                let gate = Arc::clone(&gate);
                tokio::spawn(async move {
                    let c = ctx(tenant, user, &[]);
                    gate.wait().await;
                    let own_seen = s.get_fact(&c, own).await.expect("read own").is_some();
                    let foreign_seen = s
                        .get_fact(&c, foreign)
                        .await
                        .expect("read foreign")
                        .is_some();
                    (tenant, own_seen, foreign_seen)
                })
            };

        let a = task("acme", "u-a", "fact:acme_only", "fact:globex_only");
        let b = task("globex", "u-b", "fact:globex_only", "fact:acme_only");

        for h in [a, b] {
            let (tenant, own_seen, foreign_seen) = h.await.expect("join");
            assert!(own_seen, "`{tenant}` must see its own fact");
            assert!(
                !foreign_seen,
                "RISK-012: `{tenant}` read a fact belonging to the other tenant — namespace selection \
                 is not bound to the statement"
            );
        }
    }

    /// Load-shaped regression guard: the same defect reached through the public API only.
    ///
    /// Four tenants write concurrently on a multi-threaded runtime. Verification runs afterwards and
    /// sequentially, so the check itself is race-free and trustworthy. Both directions are asserted:
    /// every fact reaches the namespace it was written for, and no fact appears in a namespace it was
    /// not written for.
    ///
    /// A misplaced fact is detectable through `get_fact` because the seeded facts are
    /// `tenant-shared`: the read filter admits them for any user of the namespace they landed in, so a
    /// leak surfaces rather than being masked by the per-user predicate.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_multi_tenant_writes_stay_in_their_own_namespace() {
        use std::sync::Arc;

        const TENANTS: [&str; 4] = ["acme", "globex", "initech", "umbrella"];
        const PER_TENANT: usize = 25;

        let s = Arc::new(store().await);

        // Provision every namespace up front, sequentially, so the concurrent phase measures the
        // select-then-query window rather than first-touch migration.
        for t in TENANTS {
            s.ensure_tenant_namespace(t).await.expect("provision");
        }

        let mut handles = Vec::with_capacity(TENANTS.len() * PER_TENANT);
        for tenant in TENANTS {
            for i in 0..PER_TENANT {
                let s = Arc::clone(&s);
                handles.push(tokio::spawn(async move {
                    let f = sample_fact(
                        &format!("fact:{tenant}_{i}"),
                        scope_ref(tenant, None, "u-1"),
                        Visibility::TenantShared,
                    );
                    s.put_fact(&f).await
                }));
            }
        }
        for h in handles {
            h.await.expect("join").expect("put_fact");
        }

        // Sequential verification — no race in the check itself.
        let mut misplaced: Vec<String> = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        for tenant in TENANTS {
            let c = ctx(tenant, "u-1", &[]);
            for i in 0..PER_TENANT {
                let own = format!("fact:{tenant}_{i}");
                if s.get_fact(&c, &own).await.unwrap().is_none() {
                    missing.push(format!("{own} absent from `{tenant}`"));
                }
                for other in TENANTS.iter().filter(|o| **o != tenant) {
                    let foreign = format!("fact:{other}_{i}");
                    if s.get_fact(&c, &foreign).await.unwrap().is_some() {
                        misplaced.push(format!("{foreign} found in `{tenant}`"));
                    }
                }
            }
        }

        assert!(
            misplaced.is_empty() && missing.is_empty(),
            "RISK-012: concurrent multi-tenant writes crossed namespaces.\n  \
             misplaced ({}): {:?}\n  missing ({}): {:?}",
            misplaced.len(),
            misplaced.iter().take(10).collect::<Vec<_>>(),
            missing.len(),
            missing.iter().take(10).collect::<Vec<_>>()
        );
    }

    // ================================================================================================
    // SPIKE (RFC 03, D1 option 1) — can a statement carry its own namespace, so no shared session
    // state exists to race on?
    //
    // The SDK exposes no per-query ns/db scoping (`use_ns`/`use_db` are the only entry points and they
    // mutate the connection session). But `select_namespace` already proves `USE` works INSIDE a query
    // batch: it sends `DEFINE NAMESPACE ...; USE NS ...; DEFINE DATABASE ...` as one call and the
    // DEFINE DATABASE lands in the right namespace.
    //
    // So the question is whether a single `query()` batch is atomic with respect to other tasks. If it
    // is, then prefixing every statement with its own `USE NS <t> DB memory;` in the SAME call makes
    // each operation self-contained: a concurrent task's USE cannot land between this batch's USE and
    // its statement, because they travel as one request.
    // ================================================================================================

    /// Spike: `USE NS ...; <statement>` in one batch, hammered concurrently across two tenants.
    ///
    /// PASSES  => option 1 is viable; the D1 fix is to fold the namespace into every statement batch
    ///            and delete the standalone `use_tenant` call. No lock, no pool, no per-tenant
    ///            connection.
    /// FAILS   => the batch is not atomic; fall back to a namespace-keyed connection pool.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn spike_use_prefix_in_one_batch_is_atomic() {
        use std::sync::Arc;

        const ROUNDS: usize = 60;

        let s = Arc::new(store().await);

        // Seed one distinguishable tenant-shared fact per namespace, sequentially.
        s.put_fact(&sample_fact(
            "fact:acme_only",
            scope_ref("acme", None, "u-a"),
            Visibility::TenantShared,
        ))
        .await
        .unwrap();
        s.put_fact(&sample_fact(
            "fact:globex_only",
            scope_ref("globex", None, "u-b"),
            Visibility::TenantShared,
        ))
        .await
        .unwrap();

        // Concurrent phase: every task issues ONE batch that carries its own namespace.
        let mut handles = Vec::with_capacity(ROUNDS * 2);
        for round in 0..ROUNDS {
            for tenant in ["acme", "globex"] {
                let s = Arc::clone(&s);
                handles.push(tokio::spawn(async move {
                    let sql = format!(
                        "USE NS {tenant} DB {DB_NAME}; SELECT meta::id(id) AS fid FROM fact"
                    );
                    let mut resp = s.db.query(sql).await.expect("batch query");
                    // statement 0 is the USE; the SELECT's rows are in slot 1.
                    let rows: Vec<Json> = resp.take(1).expect("rows");
                    let seen: Vec<String> = rows
                        .iter()
                        .filter_map(|r| r.get("fid").and_then(|v| v.as_str()).map(str::to_string))
                        .collect();
                    (tenant, round, seen)
                }));
            }
        }

        let mut wrong: Vec<String> = Vec::new();
        for h in handles {
            let (tenant, round, seen) = h.await.expect("join");
            let expected = if tenant == "acme" {
                "acme_only"
            } else {
                "globex_only"
            };
            if seen != vec![expected.to_string()] {
                wrong.push(format!("round {round} tenant `{tenant}` saw {seen:?}"));
            }
        }

        assert!(
            wrong.is_empty(),
            "SPIKE RESULT: a `USE NS ...; <statement>` batch is NOT atomic under concurrency — \
             {} of {} batches read the wrong namespace. Option 1 is not viable; fall back to a \
             namespace-keyed connection pool.\n  first failures: {:?}",
            wrong.len(),
            ROUNDS * 2,
            wrong.iter().take(8).collect::<Vec<_>>()
        );
    }

    /// Spike, write side: the SAME workload that fails 70/100 in
    /// `concurrent_multi_tenant_writes_stay_in_their_own_namespace`, re-run with the namespace folded
    /// into each statement's own batch. Four tenants, 25 concurrent writes each, 4-thread runtime.
    ///
    /// This is the decisive comparison: identical concurrency, identical shared connection, the only
    /// difference being that the namespace travels with the statement instead of being set by a
    /// preceding call.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn spike_use_prefix_batch_write_side() {
        use std::sync::Arc;

        const TENANTS: [&str; 4] = ["acme", "globex", "initech", "umbrella"];
        const PER_TENANT: usize = 25;

        let s = Arc::new(store().await);
        for t in TENANTS {
            s.ensure_tenant_namespace(t).await.expect("provision");
            // Define the probe table up front. Letting concurrent CREATEs auto-define it produces
            // transaction write conflicts on the schema record — a real behaviour, but a different
            // one from the namespace question this spike is asking.
            s.db.query(format!(
                "USE NS {t} DB {DB_NAME}; DEFINE TABLE IF NOT EXISTS spike_probe SCHEMALESS"
            ))
            .await
            .expect("define probe")
            .check()
            .expect("probe defined");
        }

        let mut handles = Vec::with_capacity(TENANTS.len() * PER_TENANT);
        for tenant in TENANTS {
            for i in 0..PER_TENANT {
                let s = Arc::clone(&s);
                handles.push(tokio::spawn(async move {
                    // One batch: the namespace and the write travel together.
                    let sql = format!(
                        "USE NS {tenant} DB {DB_NAME}; \
                         CREATE spike_probe SET tenant = $t, seq = $i"
                    );
                    s.db.query(sql)
                        .bind(("t", tenant.to_string()))
                        .bind(("i", i as i64))
                        .await
                        .expect("batch write")
                        .check()
                        .expect("statement ok");
                }));
            }
        }
        for h in handles {
            h.await.expect("join");
        }

        // Sequential verification: every probe row must carry the tenant of the namespace it sits in.
        let mut misplaced: Vec<String> = Vec::new();
        let mut counts: Vec<(String, usize)> = Vec::new();
        for tenant in TENANTS {
            let mut resp =
                s.db.query(format!(
                    "USE NS {tenant} DB {DB_NAME}; SELECT tenant, seq FROM spike_probe"
                ))
                .await
                .expect("verify");
            let rows: Vec<Json> = resp.take(1).expect("rows");
            counts.push((tenant.to_string(), rows.len()));
            for r in &rows {
                let owner = r.get("tenant").and_then(|v| v.as_str()).unwrap_or("?");
                if owner != tenant {
                    misplaced.push(format!(
                        "row written for `{owner}` found in namespace `{tenant}`"
                    ));
                }
            }
        }

        assert!(
            misplaced.is_empty(),
            "SPIKE RESULT (write side): namespace-qualified batches still crossed namespaces — \
             {} misplaced.\n  {:?}\n  counts: {:?}",
            misplaced.len(),
            misplaced.iter().take(8).collect::<Vec<_>>(),
            counts
        );
        for (tenant, n) in &counts {
            assert_eq!(
                *n, PER_TENANT,
                "namespace `{tenant}` should hold exactly {PER_TENANT} probe rows, holds {n} \
                 (counts: {counts:?})"
            );
        }
    }
}
