//! Provider & infrastructure traits (§2C.6) — dependency-injected seams that keep providers
//! swappable (OQ-MODELS/OQ-QUEUE/OQ-STORE).
//!
//! The traits are DECLARED here; component logic that implements them lands in later phases.
//! `StoreError`/`Candidate`/`StageOneQuery`/`AuditEntry` are owned by the C1 spec; `QueueError`
//! by C2; `ExtractedFact`/`EntityMention` by C4. (recall is LLM-free — ADR-015 — so there is no
//! `LlmClient` and no `InsightCandidate`.) Each component phase refines its owned type in place.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::api::{DeletionProof, SubQueryKind, TenantErasureProof};
use crate::types::domain::{Entity, Fact, MemoryClass, Relationship, Source, Visibility};
use crate::types::job::{JobKind, WorkJob};
use crate::types::scope::{ScopeContext, ScopeRef};
use crate::write_pipeline::QuarantineRecord;

// --- Store-owned supporting types (C1) — canonical, per the C1 Memory Store spec (Phase 2) ---

/// Typed error for every `MemoryStore` operation. Maps to `AppError::Store` in §2C.7; the
/// `StoreError` -> error-code mapping is owned by the Phase 4 X1 registry (`src/error.rs`).
#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    /// Record failed domain validation. `VAL_INVALID_BODY` for structural failures,
    /// `VAL_OUT_OF_RANGE` for score-range failures (-> 400).
    #[error("validation: {0}")]
    Validation(String),
    /// Target id absent or excluded by the read filter (-> 404 NOT_FOUND).
    #[error("not found")]
    NotFound,
    /// A `put_*` record's `owner.tenant` differs from the active namespace, its id already belongs to
    /// another user, or a readable record is changed or removed by someone who does not own it
    /// (-> 403 SCOPE_FORBIDDEN). Never retried: the same write can only be refused again.
    #[error("scope forbidden")]
    ScopeForbidden,
    /// A recall with no collection filter has more than 32 collections in play (-> 400
    /// VAL_TOO_MANY_COLLECTIONS). Never retried, and no collection is dropped to fit.
    // spec: components/memory-store#SA-COLL-02
    #[error("{in_play} collections in play, at most 32")]
    TooManyCollections { in_play: u32 },
    /// Store connection lost / engine not reachable (-> 503 STORE_UNAVAILABLE).
    #[error("store unavailable: {0}")]
    Unavailable(String),
    /// Per-statement timeout (-> 504 STORE_TIMEOUT).
    #[error("store timeout")]
    Timeout,
    /// `hard_delete` verification found a collected record still present (-> 500 INTERNAL).
    #[error("partial delete: {removed} of {expected} removed")]
    PartialDelete { removed: u32, expected: u32 },
    /// An unbounded namespace-level destructive operation was refused because the namespace may hold
    /// tenants besides the named one (-> 403 SCOPE_FORBIDDEN). See `erase_tenant` for the bounded
    /// path that is always safe (ADR-018).
    #[error("namespace {namespace} may hold tenants other than {tenant}")]
    NamespaceNotExclusive { tenant: String, namespace: String },
    /// Unhandled internal invariant violation (-> 500 INTERNAL).
    #[error("internal: {0}")]
    Internal(String),
}

/// One stage-1 retrieval candidate: a fact id, the fact itself, and the per-signal scores that
/// contributed to its selection. The Retrieval Engine (C6) fuses and reranks these.
#[derive(Clone)]
pub struct Candidate {
    /// "fact:<uuidv7>".
    pub fact_id: String,
    /// The resolved fact, already read-filtered.
    pub fact: Fact,
    /// Cosine similarity from the HNSW vector index, [0,1]; 0.0 if not a vector hit.
    pub semantic_score: f64,
    /// BM25 score, normalised to [0,1]; 0.0 if not a keyword hit.
    pub keyword_score: f64,
    /// Graph-proximity score, [0,1]; 0.0 if not reached via traversal.
    pub graph_score: f64,
    /// Raw reciprocal-rank-fusion sum plus the rank bonus (step 10′, SA-COLL-04).
    // spec: components/memory-store#SA-COLL-04
    pub fused_score: f64,
    /// `fused_score / fused_max`, clamped to [0,1] (step 10′).
    pub fused_norm: f64,
    /// The fact's source `trust_signal`; 1.0 when `source_id` is `None` or the source is unreadable
    /// (step 9″.7, ADR-021).
    pub source_trust: f64,
    /// 0 = the fact names a matched (or filter) entity; 1 or 2 = reached through links (step 9″).
    pub graph_hop: Option<u8>,
    /// The relationship ids that reached the fact (hop ≥ 1), at most 4 (step 9″.4).
    pub via: Vec<String>,
}

/// The stage-1 multi-signal query the Retrieval Engine (C6) submits to `memory`.
#[derive(Clone)]
pub struct StageOneQuery {
    /// `dim == MEMORY_EMBED_DIM`; empty disables the vector signal.
    pub query_vector: Vec<f32>,
    /// BM25 terms; empty disables the keyword signal.
    pub keyword_terms: Vec<String>,
    /// Typed sub-queries (C6 step 4′, ADR-020); empty for a plain query. When non-empty,
    /// `query_vector` and `keyword_terms` are empty.
    pub searches: Vec<StageOneSearch>,
    /// Metadata filters (memory_class, visibility, entity, valid_at, collections).
    pub filters: RecallFilters,
    /// The authenticated scope; the read filter is applied to every signal.
    pub scope: ScopeContext,
    /// Max candidates to return (SA-RERANK-01, default `MEMORY_STAGE1_K` = 50).
    pub stage1_k: u16,
    /// `MEMORY_GRAPH_HOPS`, 0..=2; 0 turns the link list off (ADR-021).
    pub graph_hops: u8,
    /// `MEMORY_GRAPH_FANOUT` — neighbours followed per hop.
    pub graph_fanout: u32,
    /// `MEMORY_GRAPH_CANDIDATE_CAP` — facts the graph and link lists together may contribute.
    pub graph_cap: u32,
    /// The plain query, or each sub-query's text; entity phrases are cut from these (SA-GRAPH-10).
    pub phrase_texts: Vec<String>,
}

/// One typed sub-query as C1 runs it: one ranked list of one signal (ADR-020).
#[derive(Clone)]
pub struct StageOneSearch {
    /// `Lex` | `Vec` (2C.4, owned by C6's request type).
    pub kind: SubQueryKind,
    /// `Vec` only; `len == MEMORY_EMBED_DIM`.
    pub vector: Vec<f32>,
    /// `Lex` only; the keyword terms of the sub-query text.
    pub terms: Vec<String>,
    /// 2.0 for the first sub-query, 1.0 for the others (SA-SUBQ-02).
    pub weight: f64,
}

/// Metadata filters for `memory`. The §2C.4 type re-stated with `Clone, Default` so `StageOneQuery`
/// is self-contained for C1; C6/C8 use the §2C.4 definition (`crate::types::api::RecallFilters`),
/// which is field-identical.
#[derive(Clone, Default)]
pub struct RecallFilters {
    pub memory_class: Option<MemoryClass>,
    pub visibility: Option<Visibility>,
    /// Restrict to facts touching this entity id.
    pub entity: Option<String>,
    /// As-of query into the bi-temporal history.
    pub valid_at: Option<DateTime<Utc>>,
    /// 1–32 distinct labels, each matching SA-COLL-01; "" names the default collection.
    /// Absent = every collection the caller can read, at most 32 in play (ADR-020, SA-COLL-02).
    pub collections: Option<Vec<String>>,
}

/// Append-only audit record (SA-AUDIT-01), persisted to the per-tenant `audit_log` table. Canonical
/// definition owned by the C1 spec (Phase 2).
#[derive(Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// "audit_log:<uuidv7>".
    pub id: String,
    /// Selects the namespace; never stored cross-tenant.
    pub tenant: String,
    /// `ctx.user` (OIDC subject).
    pub subject: String,
    /// "recall" | "remember" | "forget" | "get_fact" | "freshness_check".
    pub operation: String,
    /// The scope the operation targeted.
    pub scope: ScopeRef,
    /// "ok" | "denied" | "error:<CODE>".
    pub outcome: String,
    /// `ctx.token_jti` — never the token itself, never PII.
    pub token_jti: String,
    pub correlation_id: String,
    /// `ctx.client` — the calling service identity, `None` when the token carried none. Added by
    /// migration 0005; rows written before it read back as `None`, which is indistinguishable from
    /// a call that genuinely carried none, so the trail stays readable across the boundary.
    // spec: components/auth-scope#SA-AUTHZ-08
    #[serde(default)]
    pub client: Option<String>,
    /// The fact class the call attempted to assert, on `remember` only. The **label only** — never
    /// any part of `content`. `None` on every other operation and on an unclassed write.
    // spec: components/service-layer#SA-AUTHZ-04
    #[serde(default)]
    pub fact_class: Option<String>,
    /// Server-set, RFC3339 ms.
    pub at: DateTime<Utc>,
}

// --- Queue-owned supporting type (C2; minimal Phase-1 form) ---

/// Work-queue failure modes. Canonical definition owned by the C2 spec (Phase 3). Every variant maps
/// through `AppError::Queue` to HTTP `503 QUEUE_UNAVAILABLE` (X1 registry); the `Display` detail is
/// logged at `error`, never returned to a client.
#[derive(thiserror::Error, Debug)]
pub enum QueueError {
    /// Store/NATS connection or statement failure -> 503.
    #[error("queue backend unavailable: {0}")]
    BackendUnavailable(String),
    /// `complete`/`fail` referenced an unknown job id.
    #[error("job not found: {0}")]
    JobNotFound(String),
    /// `complete`/`fail` on a job not in `Leased` status.
    #[error("job not leased: {0}")]
    NotLeased(String),
    /// `enqueue` payload failed structural validation.
    #[error("invalid job: {0}")]
    InvalidJob(String),
    /// `MEMORY_QUEUE_BACKEND=nats` without `MEMORY_QUEUE_NATS_URL`.
    #[error("backend misconfigured: {0}")]
    Misconfigured(String),
}

// --- Write-pipeline supporting types (C4) — canonical, per the C4 Write Pipeline spec (Phase 5) ---

/// The LLM extractor's output shape, pre-persistence. One per asserted fact. This is NOT a stored
/// `Fact`: it carries no id, no scores, no validity, no scope — those are assigned by later pipeline
/// steps (C4 *Public Interface*).
#[derive(Clone, Serialize, Deserialize)]
pub struct ExtractedFact {
    /// Structured assertion (JSON object), not free text.
    pub content: serde_json::Value,
    /// Raw entity references to resolve (>=1). (Spec name: `entity_mentions`.)
    pub entities: Vec<EntityMention>,
    /// Proposed memory class; `procedural` is impossible (enum). (Spec name: `memory_class`.)
    pub memory_class: crate::types::domain::MemoryClass,
    /// [0,1] — the LLM's own confidence in this extraction. (Spec name: `extractor_confidence`.)
    pub confidence: f64,
}

/// A mention of an entity within extracted content (C4 *Public Interface*).
#[derive(Clone, Serialize, Deserialize)]
pub struct EntityMention {
    /// Surface form as it appeared in the content (1..=512 chars). (Spec name: `surface_form`.)
    pub surface: String,
    /// Optional coarse type hint, e.g. "person", "team". (Spec name: `mention_type`.)
    pub canonical_name: Option<String>,
}

// (InsightCandidate removed by ADR-015 — server-side consolidation is dropped; the agent writes
// consolidated insights itself as agent-stated facts.)

// --- Provider-shared error/result types (§2C.6) ---

/// Shared by EmbeddingClient/RerankClient/PiiDetector.
#[derive(thiserror::Error, Debug)]
pub enum ProviderError {
    /// -> 504 PROVIDER_TIMEOUT.
    #[error("provider timeout")]
    Timeout,
    /// -> 502 PROVIDER_ERROR.
    #[error("provider status {0}")]
    Status(u16),
    /// -> 502 PROVIDER_ERROR.
    #[error("provider transport: {0}")]
    Transport(String),
    /// -> 502 PROVIDER_ERROR.
    #[error("provider malformed: {0}")]
    Malformed(String),
}

/// A PII span returned by `PiiDetector::scan` (C4 *Public Interface*). Produced by the injected
/// `PiiDetector` trait; consumed only by the write pipeline.
#[derive(Clone, Serialize, Deserialize)]
pub struct PiiSpan {
    /// RFC 6901 pointer into `content` locating the string value containing the span.
    pub json_pointer: String,
    /// Byte offset within the located string value (inclusive start).
    pub start: u32,
    /// Exclusive byte offset; `end > start`.
    pub end: u32,
    /// e.g. "email", "person", "phone", "national_id".
    pub pii_type: String,
    /// [0,1].
    pub confidence: f64,
}

// --- Infrastructure traits ---

#[async_trait]
pub trait MemoryStore: Send + Sync {
    // --- Fact CRUD + bi-temporal ---
    /// Upsert; embedding passed alongside.
    async fn put_fact(&self, f: &Fact) -> Result<(), StoreError>;
    async fn get_fact(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Fact>, StoreError>;
    /// Multi-signal stage-1.
    async fn recall(
        &self,
        ctx: &ScopeContext,
        q: &StageOneQuery,
    ) -> Result<Vec<Candidate>, StoreError>;
    async fn end_validity(
        &self,
        ctx: &ScopeContext,
        id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError>;
    async fn supersede(
        &self,
        ctx: &ScopeContext,
        old_id: &str,
        new_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError>;
    async fn hard_delete(&self, ctx: &ScopeContext, id: &str) -> Result<DeletionProof, StoreError>;
    /// After an erasure commits: remove what still summarises the `gone` facts: stored summaries,
    /// held summaries and dead letters: the caller's own under a user's scope, and those of the
    /// scope's team under a maintenance scope. Returns how many stored summaries went.
    // A summary committing while the delete runs is missed by the delete's own read, and held and
    // failed work is outside its transaction.
    async fn sweep_summaries(&self, ctx: &ScopeContext, gone: &[String])
        -> Result<u32, StoreError>;
    // --- Entity CRUD + resolution support ---
    /// Upsert.
    async fn put_entity(&self, e: &Entity) -> Result<(), StoreError>;
    async fn get_entity(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Entity>, StoreError>;
    async fn find_entity_by_name(
        &self,
        ctx: &ScopeContext,
        name: &str,
    ) -> Result<Vec<Entity>, StoreError>;
    async fn merge_entities(
        &self,
        ctx: &ScopeContext,
        keep_id: &str,
        merge_id: &str,
    ) -> Result<(), StoreError>;
    // --- Relationship CRUD ---
    async fn put_relationship(&self, r: &Relationship) -> Result<(), StoreError>;
    async fn get_relationship(
        &self,
        ctx: &ScopeContext,
        id: &str,
    ) -> Result<Option<Relationship>, StoreError>;
    async fn end_relationship_validity(
        &self,
        ctx: &ScopeContext,
        id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError>;
    // --- Source CRUD ---
    /// Upsert.
    async fn put_source(&self, s: &Source) -> Result<(), StoreError>;
    async fn get_source(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Source>, StoreError>;
    /// The owner of the fact, entity, relationship or source `id` names, read past the read filter so
    /// an import can refuse an id it may not see. `None` when no such row exists.
    // No other operation tells "absent" from "unreadable", which a write naming its own id needs. The
    // owner never reaches a caller; the import reports only that the id is refused.
    async fn record_owner(&self, tenant: &str, id: &str) -> Result<Option<ScopeRef>, StoreError>;
    // --- Audit (SA-AUDIT-01) ---
    /// Append-only, synchronous.
    async fn append_audit(&self, e: &AuditEntry) -> Result<(), StoreError>;
    // --- Maintenance surface consumed by C7 (all take ctx; ctx.tenant selects the namespace) ---
    /// Admin op, no ctx.
    async fn list_tenants(&self) -> Result<Vec<String>, StoreError>;
    async fn scan_recent_episodes(
        &self,
        ctx: &ScopeContext,
        since: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError>;
    async fn scan_contradiction_candidates(
        &self,
        ctx: &ScopeContext,
        limit: u32,
    ) -> Result<Vec<(Fact, Fact)>, StoreError>;
    /// Coarse prefilter; C7 owns the decay maths.
    async fn scan_decay_candidates(
        &self,
        ctx: &ScopeContext,
        salience_floor: f64,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError>;
    async fn scan_reembed_candidates(
        &self,
        ctx: &ScopeContext,
        current_model_version: &str,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError>;
    async fn update_fact_maintenance_fields(
        &self,
        ctx: &ScopeContext,
        f: &Fact,
    ) -> Result<(), StoreError>;
    async fn set_fact_embedding(
        &self,
        ctx: &ScopeContext,
        fact_id: &str,
        vector: &[f32],
        model_version: &str,
    ) -> Result<(), StoreError>;
    // --- Lifecycle / tenancy ---
    /// Idempotent.
    async fn ensure_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>;
    /// Per-tenant erasure (ADR-018): a predicate-scoped delete of every row carrying `tenant`,
    /// across every table in the namespace it occupies, verified by a post-delete re-count. Correct
    /// whether the tenant has the namespace to itself or shares it.
    async fn erase_tenant(&self, tenant: &str) -> Result<TenantErasureProof, StoreError>;
    /// Drop the whole namespace — the ADR-011 single-tenant-namespace mechanism, and unbounded by
    /// any predicate. Refuses with `StoreError::NamespaceNotExclusive` where the namespace may hold
    /// other tenants; `erase_tenant` is the path that always holds (ADR-018).
    async fn drop_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>;
    /// Connection live + vector-index dim == MEMORY_EMBED_DIM.
    async fn ready(&self) -> Result<(), StoreError>;
    // --- Scope listing and team knowledge (ordin changes M-6 b, M-7) ---
    // No other operation pages a scope's facts, reads relationships by endpoint or sets `pii_review`
    // alone (`put_fact` replaces the whole row and clears the embedding). These four are the smallest
    // store surface `Service::list_facts`, `export_scope` and `clear_pii_review` need.
    /// Read-filtered facts, newest `ingested_at` first (then id, descending), strictly after the
    /// keyset `after = (ingested_at, id)` of the previous page's last fact, at most `limit`. Facts
    /// whose validity has ended are left out unless `include_retired`.
    async fn list_facts_page(
        &self,
        ctx: &ScopeContext,
        after: Option<(DateTime<Utc>, String)>,
        include_retired: bool,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError>;
    /// Read-filtered facts of every validity ordered by id, strictly after `after_id`, at most
    /// `limit`.
    async fn export_facts_page(
        &self,
        ctx: &ScopeContext,
        after_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError>;
    /// Read-filtered relationships of every validity whose `from` and `to` are both in `entity_ids`.
    async fn relationships_among(
        &self,
        ctx: &ScopeContext,
        entity_ids: &[String],
    ) -> Result<Vec<Relationship>, StoreError>;
    /// Set `pii_review` on one readable fact, leaving every other field and the embedding as they
    /// are. `StoreError::NotFound` when the fact is absent or not readable.
    async fn set_pii_review(
        &self,
        ctx: &ScopeContext,
        id: &str,
        pii_review: bool,
    ) -> Result<(), StoreError>;

    // --- Triples and conflicts (memory-store amendment ADR-022; ordin change M-10 c) ---
    /// The live facts of `owner`, of every visibility and collection, with this subject entity and
    /// predicate, newest `valid_from` first (then id, descending), at most `limit` (1..=64). Served by
    /// the index `fact_triple` (ADR-022 (4)).
    // spec: components/memory-store#SA-TRIPLE-08
    async fn live_same_subject_predicate(
        &self,
        ctx: &ScopeContext,
        owner: &ScopeRef,
        subject_entity: &str,
        predicate: &str,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError>;
    /// Upsert one conflict pair (ordered by id); a pair where either fact is absent or not live is not
    /// written (`Ok(false)`); a pair of two owners is refused.
    async fn record_conflict(
        &self,
        ctx: &ScopeContext,
        a: &str,
        b: &str,
        at: DateTime<Utc>,
    ) -> Result<bool, StoreError>;
    /// The conflict rows naming any of `ids` (1..=100) whose other fact the caller can read and is
    /// live: one item per id of `ids` on a row, ordered by `fact`, then `with`.
    async fn conflicts_of(
        &self,
        ctx: &ScopeContext,
        ids: &[String],
    ) -> Result<Vec<FactConflict>, StoreError>;
}

/// One side of a recorded conflict as the store returns it (memory-store amendment ADR-022).
// spec: phase-1-analysis#SA-TRIPLE-13
#[derive(Clone, Debug, PartialEq)]
pub struct FactConflict {
    pub fact: String,
    pub with: String,
    pub recorded_at: DateTime<Utc>,
}

// FreshnessChecker removed by ADR-014 — freshness is agent-side; memory runs no read-path check.

#[async_trait]
pub trait WorkQueue: Send + Sync {
    /// Returns job id. The namespace comes from `job.scope.tenant`.
    async fn enqueue(&self, job: WorkJob) -> Result<String, QueueError>;
    /// Claim one job of any listed kind from `tenant`'s queue.
    ///
    /// `tenant` is explicit rather than implied by connection state (RISK-016). A queue backed by the
    /// C1 store shares one SurrealDB connection with it, so an implied namespace made a claim depend
    /// on whichever tenant last touched that session — unclaimable for a tenant that had never
    /// enqueued on it, and redirectable mid-poll by a concurrent enqueue for another tenant.
    /// A consumer serving many tenants enumerates them and claims per tenant.
    async fn claim(
        &self,
        tenant: &str,
        kinds: &[JobKind],
        lease: Duration,
    ) -> Result<Option<WorkJob>, QueueError>;
    /// Mark a leased job done. `tenant` must be the claimed job's `scope.tenant`.
    async fn complete(&self, tenant: &str, job_id: &str) -> Result<(), QueueError>;
    /// Fail a leased job, retrying with backoff or dead-lettering. `tenant` as for `complete`.
    /// A dead-lettered `ExtractFact` job not marked redacted loses its text first (SA-QUEUE-03).
    async fn fail(&self, tenant: &str, job_id: &str, retryable: bool) -> Result<(), QueueError>;
    /// Replace a leased job's payload with its redacted form, just before `fail` parks it.
    // spec: phase-1-analysis#SA-QUEUE-03
    async fn redact_job(
        &self,
        tenant: &str,
        job_id: &str,
        payload: &serde_json::Value,
    ) -> Result<(), QueueError>;
    /// Record that the job is about to write the fact, so `hard_delete` can find the job. Repeating it
    /// changes nothing; the link goes when the job completes.
    async fn link_job_fact(
        &self,
        scope: &ScopeRef,
        job_id: &str,
        fact_id: &str,
    ) -> Result<(), QueueError>;
    /// Drop the job's links: a worker taking back its own write keeps its job.
    async fn unlink_job_facts(&self, tenant: &str, job_id: &str) -> Result<(), QueueError>;
    /// Whether the job's row still exists. An erasure removes the jobs it covers first, so a worker
    /// that reads `false` here was overtaken and undoes what it wrote. On PostgreSQL the read waits for
    /// an erasure holding the row.
    async fn job_is_live(&self, tenant: &str, job_id: &str) -> Result<bool, QueueError>;
    /// Take back a write its owner no longer wants, at whatever stage it has reached: the job with
    /// its raw payload, its parked copy and its links, and the record the gate held it under. A worker
    /// that still holds the job finds it gone and takes its write back. A finished job keeps its row,
    /// which holds no payload and only dedupes. `scope` must be the job's own; any other scope, or an
    /// id naming no job, removes nothing. True when something was removed.
    async fn cancel_job(&self, scope: &ScopeRef, job_id: &str) -> Result<bool, QueueError>;
}

// --- The storage port (ADR-101 item 1) ---
//
// `MemoryStore` and `WorkQueue` keep their signatures. The traits below cover what previously reached
// SurrealDB directly — the idempotency records, the quarantine sink, the maintenance bookkeeping and
// the queue's lease sweep — so the components above the store hold one `Arc<dyn StoragePort>` and name
// no storage engine.

/// One stored write outcome per `(tenant, user, route, idempotency_key)`, replayed verbatim until
/// `expires_at` (SA-IDEM-01).
#[derive(Clone, Debug)]
pub struct IdempotencyRecord {
    pub tenant: String,
    pub user: String,
    pub route: String,
    pub idempotency_key: String,
    pub response_body: serde_json::Value,
    pub status_code: u16,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    async fn idempotency_get(
        &self,
        tenant: &str,
        user: &str,
        route: &str,
        key: &str,
    ) -> Result<Option<IdempotencyRecord>, StoreError>;
    async fn idempotency_put(&self, rec: &IdempotencyRecord) -> Result<(), StoreError>;
    async fn idempotency_prune(&self, now: DateTime<Utc>) -> Result<u64, StoreError>;
}

#[async_trait]
pub trait QuarantineStore: Send + Sync {
    /// Upsert by `idempotency_key` when present (replay-safe), else insert.
    async fn put_quarantine(&self, rec: &QuarantineRecord) -> Result<(), StoreError>;
    /// Remove the held row `put_quarantine` wrote for `rec`: a job overtaken by an erasure undoing it.
    async fn drop_quarantine(&self, rec: &QuarantineRecord) -> Result<(), StoreError>;
    /// One team's held writes, newest first, for a person to review. `team = None`: rows of no team.
    // spec: phase-1-analysis#SA-QUAR-01
    async fn held_writes(
        &self,
        tenant: &str,
        team: Option<&str>,
        limit: u32,
    ) -> Result<Vec<QuarantineRecord>, StoreError>;
    /// Removes what is held for a fact of a team: a forgotten fact's text does not outlive it.
    // spec: phase-1-analysis#SA-QUAR-01
    async fn drop_held_of_fact(
        &self,
        tenant: &str,
        team: Option<&str>,
        fact_id: &str,
    ) -> Result<u64, StoreError>;
    /// One held write of a team, or `None`.
    async fn held_write(
        &self,
        tenant: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Option<QuarantineRecord>, StoreError>;
    /// Remove `tenant`'s held writes quarantined before `before`; returns how many went.
    async fn expire_held(&self, tenant: &str, before: DateTime<Utc>) -> Result<u64, StoreError>;
}

#[async_trait]
pub trait MaintenanceStateStore: Send + Sync {
    async fn maintenance_state(
        &self,
        tenant: &str,
    ) -> Result<Option<(Option<DateTime<Utc>>, Option<String>)>, StoreError>;
    async fn set_maintenance_state(
        &self,
        tenant: &str,
        last_cycle_at: DateTime<Utc>,
        embed_model_version: &str,
    ) -> Result<(), StoreError>;
}

#[async_trait]
pub trait QueueMaintenance: Send + Sync {
    /// Return expired leases to `pending`; returns the count.
    async fn reap_expired_leases(&self, now: DateTime<Utc>) -> Result<u64, QueueError>;
    async fn count_dead_letter(&self, tenant: &str) -> Result<u64, QueueError>;
    /// Remove `tenant`'s dead letters parked before `before`, the failed jobs they copy and their
    /// links; returns how many dead letters went.
    // spec: phase-1-analysis#SA-QUEUE-03
    async fn expire_dead_letters(
        &self,
        tenant: &str,
        before: DateTime<Utc>,
    ) -> Result<u64, QueueError>;
    /// The jobs of one owner team not yet finished: `pending` or `leased`.
    // spec: phase-1-analysis#SA-QUEUE-04
    async fn open_jobs(&self, tenant: &str, team: Option<&str>) -> Result<u64, QueueError>;
    /// Remove `tenant`'s `done` jobs created before `before`; returns how many went.
    // spec: phase-1-analysis#SA-QUEUE-05
    async fn purge_done_jobs(&self, tenant: &str, before: DateTime<Utc>)
        -> Result<u64, QueueError>;
}

/// Erasure of one owner team within a tenant (ordin change M-6 c).
#[async_trait]
pub trait ScopeErasure: Send + Sync {
    /// A predicate delete of every row of `tenant` whose owner team is `team`, across every table
    /// that carries one, verified by a re-count on the same predicate that must find nothing. The
    /// proof's `namespace` is `team`; an incomplete erasure is `StoreError::PartialDelete`, never a
    /// proof.
    async fn erase_scope(&self, tenant: &str, team: &str)
        -> Result<TenantErasureProof, StoreError>;
}

/// Erasure of one collection of one team scope (ADR-020; ordin change M-8 e). Implemented by C1's
/// store and by the SQL storage.
#[async_trait]
pub trait CollectionErasure: Send + Sync {
    /// Predicate-scoped erasure of one collection of one team scope (ADR-020; caller: ordin's
    /// Memory Service when a workspace stops reading a team folder). Removes every fact, entity,
    /// relationship and source with owner.tenant = tenant, owner.team = team and
    /// collection = collection, except an entity or source a remaining fact still names and a
    /// relationship between two remaining entities. `collection` "" is refused.
    async fn erase_collection(
        &self,
        tenant: &str,
        team: &str,
        collection: &str,
    ) -> Result<TenantErasureProof, StoreError>;
}

// --- Entities, links and graph recall (memory-store amendment January 2026, ADR-021) ---

/// The role a fact plays for a relationship: it asserted the link, or it ended it.
// spec: components/memory-store#SA-GRAPH-04
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum LinkRole {
    Asserted,
    Ended,
}

impl LinkRole {
    /// The stored form (`relationship_fact.role`).
    pub fn as_str(self) -> &'static str {
        match self {
            LinkRole::Asserted => "asserted",
            LinkRole::Ended => "ended",
        }
    }

    /// Parse the stored form.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "asserted" => Some(LinkRole::Asserted),
            "ended" => Some(LinkRole::Ended),
            _ => None,
        }
    }
}

/// An entity `neighbours` reached: its lowest hop and one relationship that reached it.
#[derive(Clone, Debug, PartialEq)]
pub struct Neighbour {
    pub entity_id: String,
    pub hop: u8,
    /// Relationship id.
    pub via: String,
}

/// One entity of a `list_entities` page with its counts.
#[derive(Serialize, Clone)]
pub struct EntitySummary {
    pub entity: Entity,
    /// Currently-valid readable facts naming it.
    pub fact_count: u32,
    /// Live readable relationships touching it.
    pub link_count: u32,
}

/// A page of `list_entities`: by canonical name, then id; 100 a page.
#[derive(Serialize, Clone)]
pub struct EntityPage {
    pub items: Vec<EntitySummary>,
    pub next_cursor: Option<String>,
}

/// One entity with its currently-valid facts and its relationships, live and ended.
#[derive(Serialize, Clone)]
pub struct EntityDetail {
    pub entity: Entity,
    pub facts: Vec<Fact>,
    pub links: Vec<Relationship>,
    /// Every link endpoint id to its canonical name.
    pub names: std::collections::HashMap<String, String>,
}

/// What forgetting an entity would remove: readable facts naming it and readable relationships
/// touching it, any validity.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct EntityCount {
    pub facts: u32,
    pub links: u32,
}

/// Evidence that an entity and everything about it was erased (step 20i).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EntityErasureProof {
    pub erased_at: DateTime<Utc>,
    pub entity_id: String,
    pub facts_removed: u32,
    pub links_removed: u32,
    pub derived_removed: u32,
    /// SHA-256 hex over the newline-joined sorted removed ids.
    pub digest: String,
    /// Held writes removed with the entity (the ids join the digest).
    #[serde(default)]
    pub held_removed: u32,
    /// Queued and failed jobs removed with the entity (the ids join the digest).
    #[serde(default)]
    pub jobs_removed: u32,
    /// `false`: another user's fact or link still uses the entity, so it stays for them.
    // spec: phase-1-analysis#SA-GRAPH-13
    #[serde(default)]
    pub entity_removed: bool,
    /// Facts of other users still naming the entity, readable by the caller or not. A count only.
    #[serde(default)]
    pub facts_remaining: u32,
    /// Relationships of other users still touching the entity, readable by the caller or not.
    #[serde(default)]
    pub links_remaining: u32,
}

/// The graph the store declares, written and read (memory-store amendment January 2026). Implemented by
/// the SurrealDB store and by the SQL storage; part of the `StoragePort` supertrait.
// spec: components/memory-store#SA-GRAPH-05
#[async_trait]
pub trait GraphStore: Send + Sync {
    /// A live relationship `from –kind→ to` of the write's team scope the caller can read, its own
    /// first. One it cannot read behaves as absent: the write gets a row of its own.
    // Two owners may hold live rows of one `from –kind→ to`.
    async fn find_live_relationship(
        &self,
        ctx: &ScopeContext,
        from: &str,
        kind: &str,
        to: &str,
    ) -> Result<Option<Relationship>, StoreError>;
    /// The distinct kinds of the caller's readable relationships, most recently ingested first.
    async fn relationship_kinds(
        &self,
        ctx: &ScopeContext,
        limit: u32,
    ) -> Result<Vec<String>, StoreError>;
    /// Record that `fact_id` plays `role` for `relationship_id` (upsert).
    async fn link_fact(
        &self,
        ctx: &ScopeContext,
        relationship_id: &str,
        fact_id: &str,
        role: LinkRole,
    ) -> Result<(), StoreError>;
    /// Whether a teammate could read the relationship: some fact recording it is not private, or it is
    /// a person's link not held to its owner.
    async fn link_is_shared(&self, ctx: &ScopeContext, id: &str) -> Result<bool, StoreError>;
    /// The read-filtered relationships a fact asserted or ended, with the role.
    async fn links_of_fact(
        &self,
        ctx: &ScopeContext,
        fact_id: &str,
    ) -> Result<Vec<(Relationship, LinkRole)>, StoreError>;
    /// Readable entities one of whose normalised names equals one of `phrases`.
    async fn match_entities(
        &self,
        ctx: &ScopeContext,
        phrases: &[String],
        collections: Option<&[String]>,
        limit: u32,
    ) -> Result<Vec<Entity>, StoreError>;
    /// Entities reachable from `seeds` through followable live links, up to `hops` (1..=2).
    async fn neighbours(
        &self,
        ctx: &ScopeContext,
        seeds: &[String],
        hops: u8,
        fanout: u32,
        valid_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<Neighbour>, StoreError>;
    /// A page of the caller's entities, by canonical name then id.
    async fn list_entities(
        &self,
        ctx: &ScopeContext,
        search: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<EntityPage, StoreError>;
    /// One readable entity with its facts and links; `None` when absent or unreadable.
    async fn entity_detail(
        &self,
        ctx: &ScopeContext,
        entity_id: &str,
        limit: u32,
    ) -> Result<Option<EntityDetail>, StoreError>;
    /// A fact's versions through `supersedes` / `superseded_by`, oldest first.
    async fn fact_versions(
        &self,
        ctx: &ScopeContext,
        fact_id: &str,
        limit: u32,
    ) -> Result<Vec<Fact>, StoreError>;
    /// Keep a fact from fading: `memory_class = semantic`, salience at least `salience_floor`.
    async fn keep_fact(
        &self,
        ctx: &ScopeContext,
        fact_id: &str,
        salience_floor: f64,
    ) -> Result<(), StoreError>;
    /// What `erase_entity` would remove, any validity.
    async fn count_entity(
        &self,
        ctx: &ScopeContext,
        entity_id: &str,
    ) -> Result<EntityCount, StoreError>;
    /// Erase the caller's facts naming an entity, their summaries and the caller's relationships
    /// touching it, and the entity unless another user's row still uses it; the proof says what stays.
    async fn erase_entity(
        &self,
        ctx: &ScopeContext,
        entity_id: &str,
    ) -> Result<EntityErasureProof, StoreError>;
    /// Delete one readable relationship and its fact rows, with proof.
    async fn delete_relationship(
        &self,
        ctx: &ScopeContext,
        relationship_id: &str,
    ) -> Result<DeletionProof, StoreError>;
    /// Write `names_norm` for up to `batch` entities lacking it; idempotent; 0 = done, and then
    /// `maintenance_state.names_backfilled_at` is set.
    async fn backfill_entity_names(&self, tenant: &str, batch: u32) -> Result<u64, StoreError>;
}

/// The storage port (ADR-101 item 1).
pub trait StoragePort:
    MemoryStore
    + WorkQueue
    + IdempotencyStore
    + QuarantineStore
    + MaintenanceStateStore
    + QueueMaintenance
    + ScopeErasure
    + CollectionErasure
    + GraphStore
{
}

impl<T> StoragePort for T where
    T: MemoryStore
        + WorkQueue
        + IdempotencyStore
        + QuarantineStore
        + MaintenanceStateStore
        + QueueMaintenance
        + ScopeErasure
        + CollectionErasure
        + GraphStore
{
}

#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    /// dim = config.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError>;
}

#[async_trait]
pub trait RerankClient: Send + Sync {
    async fn rerank(&self, query: &str, docs: &[String]) -> Result<Vec<f64>, ProviderError>;
}

// LlmClient removed by ADR-015 — recall is LLM-free: the agent extracts facts and consolidates
// insights, and submits structured agent-asserted content. The write pipeline wraps it directly.

// BrokerClient removed by ADR-014 — memory makes no outbound broker call.

#[async_trait]
pub trait PiiDetector: Send + Sync {
    async fn scan(&self, content: &serde_json::Value) -> Result<Vec<PiiSpan>, ProviderError>;
}
