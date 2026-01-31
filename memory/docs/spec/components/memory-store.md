### SPEC: Memory Store

**File:** `src/store` | **Package:** `memory::store` | **Phase:** 1 | **Dependencies:** none

> **Mode:** greenfield · amended 2026-01-21 (SA-COLL-02, SA-DELETE-02, SA-GRAPH-13, step 19d) · amended January 2026 (ADR-022: triples, conflicts, SA-TRIPLE-10; Approved by the user January 2026) · amended January 2026 (SA-TRIPLE-15: a fact's author; Approved by the user January 2026)
> **derivedFromHld:** 0.13.0 · amended 2026-01-20 (as-built readings)

#### Purpose

The Memory Store is `memory`'s persistence layer and the single owner of the embedded SurrealDB
connection. It implements the `MemoryStore` trait (the contract the Write Pipeline, Retrieval Engine,
Maintenance Worker, and HTTP API Edge depend on). It persists Facts, Entities, Relationships, Sources,
and the per-tenant audit log; it enforces hard tenant isolation by targeting one SurrealDB namespace
per tenant (ADR-011); and it provides the three retrieval signals over one store — an HNSW vector
index, a BM25 keyword index, and graph traversal over relationship edges — surfaced through a single
multi-signal stage-1 `recall` operation. It records bi-temporal facts and edges (validity interval plus
ingestion time, ADR-002): supersession and retirement end a record's validity but never delete it; two
operations remove data, and each returns a verifiable proof: `hard_delete` for one fact and its derived
artefacts (`DeletionProof`, SA-DELETE-01), and `erase_tenant` for everything belonging to one tenant
(`TenantErasureProof`, ADR-018). The store also
exposes the maintenance scan queries the Maintenance Worker (C7) consumes: decay candidates,
contradiction candidates, and stale-embedding facts.

#### Approach

A single embedded SurrealDB instance backs all three retrieval signals and the graph, chosen because
SurrealDB is the only engine that gives graph + vector (HNSW) + keyword (BM25) over rich bi-temporal
edges in one in-process store (ADR-003/ADR-009), removing the client/server hop from the read-path
latency budget. The component is a thin trait implementation over `SurrealDB`: it owns the connection
lifecycle (embedded SurrealKV or RocksDB selected by `MEMORY_STORE_BACKEND`, or a remote
SurrealDB/TiKV cluster when `MEMORY_STORE_REMOTE_URL` is set) and per-tenant namespace selection, but
holds no domain policy beyond the read-filter rule and bi-temporal write rules stated below. The
internal pattern is one parameterised SurrealQL query per trait method (no string interpolation of
caller input). Two alternatives were rejected at component scope: building a separate query-builder
abstraction over SurrealQL (rejected — the trait surface is small and fixed, an abstraction adds
indirection without reuse); and caching parsed Facts in-process (rejected — the read path latency
budget is met by the embedded engine and ANN index, and a cache would complicate the eventual-consistency
contract from ADR-004). That rejection binds this SurrealDB store. The SQL storage holds a search cache
(*Search cache*, after step 9′) because its vector runs are exact scans in Rust.

#### Shared Context

The following shared types, env vars, and the read-filter rule are duplicated verbatim from Phase 2
(§2C, §2D) and Phase 1 (§1C). A reader implements from this section alone.

##### Domain entities (Phase 2C.2) — persisted by this component

```rust
// src/types/domain.rs   Used by: C1, C4, C6, C7 (and C8 for GET fact).

#[derive(Serialize, Deserialize, Clone)]
pub struct Fact {
    pub id: String,                       // "fact:<uuidv7>" (SA-ID-01)
    pub content: serde_json::Value,       // structured assertion (object), not free text
    pub entities: Vec<String>,            // entity ids this fact connects (>=1)
    pub source_id: Option<String>,        // "source:<uuidv7>" provenance, null for agent-stated
    pub memory_class: MemoryClass,
    /// The caller-owned class this fact was authorised as (SA-AUTHZ-09). Opaque to `memory` — no
    /// vocabulary, no validation, no value treated specially. `None` = written without a class.
    /// `#[serde(default)]` matters: `row_to_fact` deserialises the row straight into `Fact`, so
    /// without it every row written before this field would fail to decode.
    /// Distinct from `memory_class`, which is memory's own episodic/semantic/consolidated vocabulary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fact_class: Option<String>,
    pub visibility: Visibility,
    pub owner: ScopeRef,                  // owning (tenant, team, user)
    pub valid_from: DateTime<Utc>,        // RFC3339 ms (SA-TIME-01)
    pub valid_to: Option<DateTime<Utc>>,  // null = currently true (open interval)
    pub ingested_at: DateTime<Utc>,       // server-set
    pub confidence: f64,                  // [0,1] (SA-SCORE-01)
    pub salience: f64,                    // [0,1]
    pub stability: f64,                   // decay stability `s` (SA-DECAY-01), >=0.0
    pub pii_review: bool,                 // true => low-confidence PII flagged for review (SA-PII-01)
    pub supersedes: Option<String>,       // fact id this one replaces
    pub superseded_by: Option<String>,    // fact id replacing this one
    pub derived_from: Vec<String>,        // source fact ids (consolidated insights only)
    pub last_recalled_at: Option<DateTime<Utc>>,
}
// The persisted SurrealDB row additionally carries derived, persistence-only fields not on this
// JSON struct: `embedding: option<array<float>>` (HNSW vector, written by C4, refreshed by C7) and
// `embedding_model: option<string>` (version tag the C7 re-embed scan keys on). See Data Model.

#[derive(Serialize, Deserialize, Clone)]
pub struct Entity {
    pub id: String,                       // "entity:<uuidv7>"
    pub canonical_name: String,           // 1..=512 chars, non-empty
    pub aliases: Vec<String>,
    pub owner: ScopeRef,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Relationship {
    pub id: String,                       // "relationship:<uuidv7>"
    pub kind: String,                     // typed edge label, e.g. "owns" (1..=128 chars)
    pub from: String,                     // entity id
    pub to: String,                       // entity id
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    pub ingested_at: DateTime<Utc>,
    pub confidence: f64,                  // [0,1]
    pub source_id: Option<String>,
    pub owner: ScopeRef,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Source {
    pub id: String,                       // "source:<uuidv7>"
    pub origin_ref: String,               // document/system handle, opaque to recall
    pub modification_marker: Option<String>, // ETag / Last-Modified token for freshness
    pub trust_signal: f64,                // [0,1] prior trust of this source
    pub owner: ScopeRef,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryClass { Episodic, Semantic, Consolidated }   // procedural rejected (SA-CLASS-01)

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility { UserPrivate, TeamShared, TenantShared } // (SA-VIS-01)
```

**Domain validation (binding at the store boundary):** `entities.len() >= 1`; all score fields
(`confidence`, `salience`, `trust_signal`) in `[0.0, 1.0]`; `stability >= 0.0`; `valid_to` (if present)
`>= valid_from`; `content` MUST be a JSON object; `canonical_name` 1..=512 chars non-empty; `kind`
1..=128 chars. A record that fails these is rejected with `StoreError::Validation` before any write.

##### Scope & auth context (Phase 2C.3)

```rust
// src/types/scope.rs   Used by: C3 (produces), C1/C4/C6/C7/C8 (consume — every query is scoped).

/// The owning scope stored on every record.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ScopeRef {
    pub tenant: String,            // tenant id -> SurrealDB namespace (ADR-011)
    pub team: Option<String>,      // team id, null for user-only facts
    pub user: String,              // user id, bound to the OIDC subject claim
}

/// The authenticated request context derived by C3 from the validated token.
/// Never constructed from request-body input.
#[derive(Clone)]
pub struct ScopeContext {
    pub tenant: String,
    pub teams: Vec<String>,        // membership claim — teams the user belongs to
    pub user: String,              // = token subject claim
    pub token_jti: String,         // for the audit trail (never the token itself)
    pub allowed_ops: OpSet,        // read / write / forget, from token scopes
    pub correlation_id: String,
}

#[derive(Clone, Copy)]
pub struct OpSet { pub read: bool, pub write: bool, pub forget: bool }
```

**Read filter rule (binding for every store query, copied verbatim from §2C.3):** a caller may read a
Fact iff `record.owner.tenant == ctx.tenant` **and** ( `record.owner.user ==
ctx.user` **or** (`record.visibility == TeamShared` and `record.owner.team ∈ ctx.teams`) **or**
`record.visibility == TenantShared` **or** the caller holds an **active consent grant** over that Fact — one issued by its owner, naming the caller as grantee, unrevoked, and inside its validity window (`docs/spec/components/consent-grants.md`, SA-GRANT-01). The grant arm applies to Facts only, and never to the vector-search path (SA-GRANT-08). ).

The `record.owner.tenant == ctx.tenant` conjunct is **enforced in the predicate**, not merely implied by
the namespace. It was originally omitted from the SQL on the grounds that a different tenant is a
different namespace, which made it redundant — true under ADR-011, but it left the rule resting on a
topology invariant rather than on the filter, and the shared-namespace stage (ADR-018) removes that invariant on purpose
(RISK-017). All three arms needed it, not only the `TenantShared` one: `owner.user` and `owner.team`
are unique within a tenant, not across tenants, so co-resident id collisions would match too.

Entities, Relationships and Sources have no `visibility` field, and team membership alone reads no
Entity and no asserted Relationship, so a name or a link from one user's private fact does not reach
that user's teammates:

- **Entity:** `record.owner.tenant == ctx.tenant` **and** ( `record.owner.user == ctx.user` **or** a Fact
  naming the entity passes `fact_read_filter(ctx)` ). A reader who is not the owner sees
  `Entity::as_seen_by(false)`: `aliases` without the owner's private aliases, and `private_aliases`
  and `alias_folders` empty.
- **Relationship:** `record.owner.tenant == ctx.tenant` **and** ( `record.owner.user == ctx.user` **or** a
  Fact with a `relationship_fact` row for it, either role, passes `fact_read_filter(ctx)` **or**
  (`replaces` is set, `owner_only` is false and `record.owner.team ∈ ctx.teams`) ).
- **Source:** the reduced filter `record.owner.tenant == ctx.tenant` **and** ( `record.owner.user ==
  ctx.user` **or** `record.owner.team ∈ ctx.teams` ).

`fact_read_filter` carries the grant arm, so a grantee reads the entities and links its granted Fact
records. A maintenance scope reads the whole tenant under the Entity and Relationship filters, as it
does under the fact filter. The SQL storage states the two filters as predicates (`entity_read_filter`,
`rel_read_filter`); the SurrealDB store applies them after the read (`visible_entities`,
`visible_relationships`), because a SurrealQL subquery per row costs more than one query over the rows.

**Maintenance-scope read filter (RISK-009, resolved):** the C7 Maintenance Worker constructs its
`ScopeContext` from a tenant id alone, with `token_jti == "maintenance"` and an empty `user` (never
from request input). `is_maintenance_scope(ctx)` is `ctx.token_jti == "maintenance" && ctx.user.is_empty()`.
For a maintenance scope, the per-user/visibility predicate would otherwise admit only `tenant-shared`
facts (empty `user`/`teams`), so decay, supersession, and verifiable hard-delete would silently no-op
on the bulk of memory (most facts are `user-private`). To prevent that, the fact read-filter helper
`fact_read_filter(ctx)` returns `owner.tenant = $ctenant` when `is_maintenance_scope(ctx)` holds — the
per-user/visibility arms are bypassed so the C7 maintenance scope operates on the whole tenant, but the
tenant conjunct is **not** bypassed — and otherwise returns `fact_filter_clause()`, which carries the
same conjunct.

The maintenance branch returned the literal `true` until RISK-017. That was safe only while tenant
isolation was structural, and it is the more dangerous of the two cases: a maintenance scope mutates
what it reads, so a co-resident leak there is a cross-tenant *write* rather than a disclosure. Note
also that namespace selection is no longer session state — every statement carries its own
`USE NS <t> DB memory;` via `Migrator::scoped` (RISK-012), and `ensure_provisioned` provisions without
selecting — so the structural argument no longer has a session to rest on either.

`fact_read_filter` is applied by `get_fact` and by `hard_delete`'s derived-collection query; the
`memory` signals use `fact_filter_clause()` directly (a maintenance scope does not drive interactive
recall). The reduced no-visibility filter `novis_filter_clause()`
(`owner.tenant = $ctenant AND (owner.user = $cuser OR owner.team IN $cteams)`) is used for `source`
reads, and for the first predicate of a neighbour hop, which adds SA-GRAPH-05's asserting-fact check
(`neighbours`). The SQL storage takes no `MemoryProfile` (`SqlStorage::new(pool, embed_dim)`): its
stage-1 signals use `search_read_filter`, the collective profile's filter without the grant arm, and
the individual profile's owner-only filter is not offered there.

##### DeletionProof (Phase 2C.4) — returned by `hard_delete`

```rust
#[derive(Serialize)]
pub struct DeletionProof {
    pub deleted_at: DateTime<Utc>,
    pub record_id: String,
    pub derived_removed: Vec<String>,        // ids of removed derived summaries
    pub embeddings_removed: u32,
    pub digest: String,                      // sha256 hex over sorted removed ids (SA-DELETE-01)
}
```

##### Trait surface (Phase 2C.6) — implemented by this component

```rust
#[async_trait]
pub trait MemoryStore: Send + Sync {                 // impl: embedded SurrealDB (C1)
    async fn put_fact(&self, f: &Fact) -> Result<(), StoreError>;
    async fn get_fact(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Fact>, StoreError>;
    async fn recall(&self, ctx: &ScopeContext, q: &StageOneQuery)
        -> Result<Vec<Candidate>, StoreError>;        // multi-signal stage-1
    async fn end_validity(&self, ctx: &ScopeContext, id: &str, at: DateTime<Utc>)
        -> Result<(), StoreError>;
    async fn hard_delete(&self, ctx: &ScopeContext, id: &str)
        -> Result<DeletionProof, StoreError>;
    // ...entity/relationship/source CRUD, maintenance scans — full surface below.
}
```

##### Environment variables (Phase 2D) — owned or read by this component

| Variable | Type | Default | Required | Description |
|---|---|---|---|---|
| `MEMORY_STORE_PATH` | path | `./data/recall.db` | no | Embedded SurrealDB data directory (SurrealKV/RocksDB). |
| `MEMORY_STORE_REMOTE_URL` | url | _(unset)_ | no | If set, target a remote SurrealDB/TiKV cluster instead of embedding (ADR-009 scale-out). |
| `MEMORY_STORE_REMOTE_USER` | string | _(unset)_ | no | Root username for a **secured** remote store; signin fires only when both user and pass are set (FU-019). Exactly one of user/pass set fails startup. |
| `MEMORY_STORE_REMOTE_PASS` | secret | _(unset)_ | no | Root password for a secured remote store (env only, redacted in logs). |
| `MEMORY_STORE_BACKEND` | enum `surrealkv\|rocksdb` | `surrealkv` | no | Embedded storage backend. |
| `MEMORY_EMBED_DIM` | u32 | `1024` | no | Embedding dimension; must equal the vector-index dimension (SA-EMBED-01). |

**Startup precedence and conditional rules (Phase 2D):** precedence is env var > config file > built-in
default. `MEMORY_STORE_REMOTE_URL` and `MEMORY_STORE_PATH` are mutually exclusive — remote wins if both
are set, with a startup warning. Startup fails fast if `MEMORY_STORE_BACKEND` is not one of
`surrealkv`/`rocksdb`, or if the persisted vector index dimension does not equal `MEMORY_EMBED_DIM`
(SA-EMBED-01 readiness check).

##### C1-owned types defined by this spec

```rust
// src/types/ports.rs   Defined here; referenced by name in §2C.6 and by C4/C6/C7.

/// Typed error for every MemoryStore operation. Maps to AppError::Store in §2C.7.
#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("validation: {0}")]                Validation(String),    // -> 400: VAL_OUT_OF_RANGE when the detail contains "range", else VAL_INVALID_BODY
    #[error("not found")]                       NotFound,              // -> 404 NOT_FOUND
    #[error("scope forbidden")]                ScopeForbidden,        // -> 403 SCOPE_FORBIDDEN
    #[error("{in_play} collections in play, at most 32")]
                                                TooManyCollections { in_play: u32 }, // -> 400 VAL_TOO_MANY_COLLECTIONS (SA-COLL-02)
    #[error("store unavailable: {0}")]          Unavailable(String),   // -> 503 STORE_UNAVAILABLE
    #[error("store timeout")]                   Timeout,               // -> 504 STORE_TIMEOUT
    #[error("partial delete: {removed} of {expected} removed")]
                                                PartialDelete { removed: u32, expected: u32 }, // -> 500 INTERNAL
    #[error("namespace {namespace} may hold tenants other than {tenant}")]
                                                NamespaceNotExclusive { tenant: String, namespace: String }, // -> 403 SCOPE_FORBIDDEN
    #[error("internal: {0}")]                   Internal(String),      // -> 500 INTERNAL
}

/// One stage-1 retrieval candidate: a fact id, the fact itself, and the per-signal scores
/// that contributed to its selection. The Retrieval Engine (C6) fuses and reranks these.
#[derive(Clone)]
pub struct Candidate {
    pub fact_id: String,            // "fact:<uuidv7>"
    pub fact: Fact,                 // the resolved fact, already read-filtered
    pub semantic_score: f64,        // cosine similarity from the HNSW vector index, [0,1]; 0.0 if not a vector hit
    pub keyword_score: f64,         // BM25 score, normalised to [0,1]; 0.0 if not a keyword hit
    pub graph_score: f64,           // graph-proximity score, [0,1]; 0.0 if not reached via traversal
}

/// The stage-1 multi-signal query the Retrieval Engine (C6) submits to `memory`.
#[derive(Clone)]
pub struct StageOneQuery {
    pub query_vector: Vec<f32>,         // dim == MEMORY_EMBED_DIM; empty disables the vector signal
    pub keyword_terms: Vec<String>,     // BM25 terms; empty disables the keyword signal
    pub filters: RecallFilters,         // metadata filters (memory_class, visibility, entity, valid_at)
    pub scope: ScopeContext,            // the authenticated scope; the read filter is applied to every signal
    pub stage1_k: u16,                  // max candidates to return (SA-RERANK-01, default MEMORY_STAGE1_K=50)
}

#[derive(Clone, Default)]
pub struct RecallFilters {
    pub memory_class: Option<MemoryClass>,
    pub visibility: Option<Visibility>,
    pub entity: Option<String>,              // restrict to facts touching this entity id
    pub valid_at: Option<DateTime<Utc>>,     // as-of query into the bi-temporal history
}
```

> Note: `RecallFilters` is the §2C.4 type re-stated here with `Clone, Default` so `StageOneQuery` is
> self-contained for C1; C6/C8 use the §2C.4 definition. The two are field-identical.

#### Public Interface

The full `MemoryStore` trait surface, expanded beyond the abbreviated §2C.6 list. All methods are
`async` and return `Result<_, StoreError>`. Every read/mutate method that takes `ctx: &ScopeContext`
selects the caller's tenant namespace and applies the read-filter rule before returning or mutating any
record. `put_fact`, `put_entity`, `put_relationship`, `put_source` are write-path (C4) entry points and
take the record's own `owner` scope; they validate `owner.tenant` against the active namespace. Each is
an upsert by id: an existing row of the same tenant and `owner.user` is replaced with its owner kept
(`put_fact` also clears the embedding), and an id that already belongs to another tenant or user is
refused with `StoreError::ScopeForbidden`, which the job loop does not retry.

**Ownership (SA-AUTHZ-07).** Reading is not owning. `end_validity`, `supersede` (both facts),
`hard_delete`, `set_pii_review`, `keep_fact`, `end_relationship_validity`, `delete_relationship`,
`erase_entity` and `merge_entities` (both entities) read their target through the read filter, where
absent or excluded is `StoreError::NotFound`, and then apply `owner_gate(ctx, owner.user)`: a caller
who is not the record's owner gets `StoreError::ScopeForbidden` and nothing changes. A maintenance
scope passes the gate (`owner_view`).

```rust
#[async_trait]
pub trait MemoryStore: Send + Sync {
    // --- Fact CRUD + bi-temporal operations ---
    async fn put_fact(&self, f: &Fact) -> Result<(), StoreError>;
    async fn get_fact(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Fact>, StoreError>;
    async fn recall(&self, ctx: &ScopeContext, q: &StageOneQuery) -> Result<Vec<Candidate>, StoreError>;
    async fn end_validity(&self, ctx: &ScopeContext, id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;
    async fn supersede(&self, ctx: &ScopeContext, old_id: &str, new_id: &str, at: DateTime<Utc>)
        -> Result<(), StoreError>; // end old validity at `at`, link old.superseded_by=new & new.supersedes=old
    async fn hard_delete(&self, ctx: &ScopeContext, id: &str) -> Result<DeletionProof, StoreError>;
    /// Called after an erasure commits (step 20r). Returns how many stored summaries went.
    async fn sweep_summaries(&self, ctx: &ScopeContext, gone: &[String]) -> Result<u32, StoreError>;

    // --- Entity CRUD + resolution support ---
    async fn put_entity(&self, e: &Entity) -> Result<(), StoreError>;   // upsert
    async fn get_entity(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Entity>, StoreError>;
    async fn find_entity_by_name(&self, ctx: &ScopeContext, name: &str)
        -> Result<Vec<Entity>, StoreError>; // exact match on canonical_name OR any alias, read-filtered
    async fn merge_entities(&self, ctx: &ScopeContext, keep_id: &str, merge_id: &str)
        -> Result<(), StoreError>; // repoint facts/relationships from merge_id to keep_id, fold aliases, drop merge_id

    // --- Relationship CRUD ---
    async fn put_relationship(&self, r: &Relationship) -> Result<(), StoreError>;
    async fn get_relationship(&self, ctx: &ScopeContext, id: &str)
        -> Result<Option<Relationship>, StoreError>;
    async fn end_relationship_validity(&self, ctx: &ScopeContext, id: &str, at: DateTime<Utc>)
        -> Result<(), StoreError>;

    // --- Source CRUD ---
    async fn put_source(&self, s: &Source) -> Result<(), StoreError>;
    async fn get_source(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Source>, StoreError>;
    /// The owner of the fact, entity, relationship or source `id` names, read past the read filter
    /// (step 20q). `None` when no such row exists.
    async fn record_owner(&self, tenant: &str, id: &str) -> Result<Option<ScopeRef>, StoreError>;

    // --- Audit trail (SA-AUDIT-01) ---
    async fn append_audit(&self, e: &AuditEntry) -> Result<(), StoreError>; // append-only, synchronous

    // --- Maintenance surface consumed by C7 (all take ctx; ctx.tenant selects the namespace) ---
    /// Currently-valid episodic facts ingested at/after `since`, capped at `limit` — consolidation input.
    async fn scan_recent_episodes(&self, ctx: &ScopeContext, since: DateTime<Utc>, limit: u32)
        -> Result<Vec<Fact>, StoreError>;
    /// Pairs of currently-valid facts sharing >=1 entity, ranked for contradiction review.
    async fn scan_contradiction_candidates(&self, ctx: &ScopeContext, limit: u32)
        -> Result<Vec<(Fact, Fact)>, StoreError>;
    /// Coarse decay prefilter: currently-valid facts with `salience < salience_floor`, ordered by
    /// staleness (oldest `last_recalled_at`/`ingested_at` first), capped at `limit`. C7 owns the
    /// Ebbinghaus retrievability maths and the final prune decision (ADR-006).
    async fn scan_decay_candidates(&self, ctx: &ScopeContext, salience_floor: f64, limit: u32)
        -> Result<Vec<Fact>, StoreError>;
    /// Currently-valid facts whose stored embedding-model version differs from `current_model_version`,
    /// or that have no embedding, eligible for re-embedding.
    async fn scan_reembed_candidates(&self, ctx: &ScopeContext, current_model_version: &str, limit: u32)
        -> Result<Vec<Fact>, StoreError>;
    /// Persist C7's maintenance bookkeeping on a fact: confidence, salience, stability,
    /// last_recalled_at, supersedes, superseded_by (scope-checked; never changes content/owner).
    async fn update_fact_maintenance_fields(&self, ctx: &ScopeContext, f: &Fact)
        -> Result<(), StoreError>;
    /// Write a refreshed embedding vector + model-version tag onto a fact's row (re-embed path).
    async fn set_fact_embedding(&self, ctx: &ScopeContext, fact_id: &str, vector: &[f32],
        model_version: &str) -> Result<(), StoreError>;

    // --- Lifecycle / tenancy ---
    async fn list_tenants(&self) -> Result<Vec<String>, StoreError>;  // enumerate provisioned namespaces (admin)
    // Inherent (not trait) constructors and seams, listed here because they are C1's public surface:
    //   pub async fn open(endpoint: &str, embed_dim: u32, profile: MemoryProfile) -> Result<Store, StoreError>
    //   pub(crate) async fn recall_vector_only(&self, ctx, q, strategy) -> Result<Vec<(String, f64)>, StoreError>
    // and on the SQL storage:
    //   pub fn new(pool: SqlPool, embed_dim: u32) -> SqlStorage          // no I/O, no profile; retry defaults 5 attempts, 2000 ms
    //   pub fn with_queue_retry(self, max_attempts: u32, backoff_base_ms: u32) -> SqlStorage   // as Store::with_queue_retry
    //   pub fn migrations(dialect: SqlDialect) -> &'static [(&'static str, &'static str)]      // the caller's migrator applies them
    async fn ensure_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>; // idempotent provision
    /// Per-tenant erasure (ADR-018): predicate-scoped delete of every row carrying `tenant`, across
    /// every table in the namespace it occupies, verified by a post-delete re-count. Correct whether
    /// the tenant has the namespace to itself or shares it.
    async fn erase_tenant(&self, tenant: &str) -> Result<TenantErasureProof, StoreError>;
    /// Namespace drop — the single-tenant-namespace mechanism only (ADR-011). Refuses with
    /// `StoreError::NamespaceNotExclusive` when the namespace may hold other tenants (ADR-018).
    async fn drop_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>;
    async fn ready(&self) -> Result<(), StoreError>; // connection live + vector-index dim == MEMORY_EMBED_DIM

    // --- Scope listing and team knowledge (steps 20m–20p; ordin changes M-6 b, M-7) ---
    async fn list_facts_page(&self, ctx: &ScopeContext, after: Option<(DateTime<Utc>, String)>,
        include_retired: bool, limit: u32) -> Result<Vec<Fact>, StoreError>;
    async fn export_facts_page(&self, ctx: &ScopeContext, after_id: Option<&str>, limit: u32)
        -> Result<Vec<Fact>, StoreError>;
    async fn relationships_among(&self, ctx: &ScopeContext, entity_ids: &[String])
        -> Result<Vec<Relationship>, StoreError>;
    async fn set_pii_review(&self, ctx: &ScopeContext, id: &str, pii_review: bool)
        -> Result<(), StoreError>;
}

/// Erasure of one owner team within a tenant (step 20s; ordin change M-6 c).
#[async_trait]
pub trait ScopeErasure: Send + Sync {
    async fn erase_scope(&self, tenant: &str, team: &str) -> Result<TenantErasureProof, StoreError>;
}
```

`AuditEntry` (C1-owned, persisted to the per-tenant `audit_log` table):

`TenantErasureProof` (C1-owned, returned by `erase_tenant`):

```rust
// src/types/api.rs   Used by: C1 (produced), operator tooling (consumed).

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TenantErasureProof {
    pub erased_at: DateTime<Utc>,       // server-set, RFC3339 ms
    pub tenant: String,                 // the logical tenant erased
    pub namespace: String,              // the physical namespace the predicate ran in
    pub removed: BTreeMap<String, u64>, // rows removed per table; ordered, so the digest is stable
    pub total_removed: u64,             // sum over `removed`
    pub digest: String,                 // sha256 hex over the digest input below (SA-ERASE-01)
}
```

The digest input is the newline-joined sequence `tenant=<tenant>`, `namespace=<namespace>`, then one
`<table>=<count>` line per table in `removed`'s ascending key order. It records what the erasure
removed; it is not a per-record commitment. A whole-tenant erasure's claim is *nothing of this tenant
remains*, and that claim is discharged by the verification pass (step 20b), which re-counts every
table on the same predicate and requires zero. Digesting every removed record id instead would hold a
25,000-fact tenant's ids in memory to prove something weaker than the re-count already proves.

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct AuditEntry {
    pub id: String,             // "audit_log:<uuidv7>"
    pub tenant: String,         // selects the namespace; never stored cross-tenant
    pub subject: String,        // ctx.user (OIDC subject)
    pub operation: String,      // "recall" | "remember" | "forget" | "get_fact" | "freshness_check"
    pub scope: ScopeRef,        // the scope the operation targeted
    pub outcome: String,        // "ok" | "denied" | "error:<CODE>"
    pub token_jti: String,      // ctx.token_jti — never the token itself, never PII
    pub correlation_id: String,
    /// ctx.client — the calling service identity, `None` when the token carried none (SA-AUTHZ-08).
    /// Added by migration 0005; rows written before it read back as `None`.
    #[serde(default)]
    pub client: Option<String>,
    /// The fact class the call attempted to assert, on `remember` only; `None` on every other
    /// operation and on an unclassed write (SA-AUTHZ-04). The **label only** — never any part of
    /// `content`. Added by migration 0005.
    #[serde(default)]
    pub fact_class: Option<String>,
    pub at: DateTime<Utc>,      // server-set, RFC3339 ms
}
```

##### Example

`put_fact` then `recall` (vector + keyword signals), within tenant `acme`:

Input to `put_fact`:

```json
{
  "id": "fact:018f9a2b-7c41-7e10-9b3a-1f2e3d4c5b6a",
  "content": {"subject": "team:alpha", "predicate": "owns", "object": "table:orders"},
  "entities": ["entity:018f9a2b-aaaa-7e10-9b3a-000000000001"],
  "source_id": "source:018f9a2b-bbbb-7e10-9b3a-000000000002",
  "memory_class": "semantic",
  "visibility": "team-shared",
  "owner": {"tenant": "acme", "team": "alpha", "user": "u-sarah"},
  "valid_from": "2026-01-04T09:00:00.000Z",
  "valid_to": null,
  "ingested_at": "2026-01-10T12:00:00.000Z",
  "confidence": 0.9, "salience": 0.7, "stability": 1.0,
  "supersedes": null, "superseded_by": null,
  "derived_from": [], "last_recalled_at": null
}
```

`recall(ctx, StageOneQuery{ query_vector: [..1024 f32..], keyword_terms: ["orders","owns"],
filters: {}, scope: ctx(tenant="acme", teams=["alpha"], user="u-sarah"), stage1_k: 50 })` returns:

```rust
vec![ Candidate {
    fact_id: "fact:018f9a2b-7c41-7e10-9b3a-1f2e3d4c5b6a".into(),
    fact: /* the Fact above */,
    semantic_score: 0.83, keyword_score: 0.61, graph_score: 0.0,
} ]
```

The candidate is returned because `owner.tenant == ctx.tenant` and `visibility == TeamShared` with
`owner.team ("alpha") ∈ ctx.teams`. A fact owned by a different tenant cannot appear: it lives in a
different namespace.

#### Internal Logic

**Connection lifecycle (constructor `Store::connect(cfg)`):**

1. Read `MEMORY_STORE_REMOTE_URL`, `MEMORY_STORE_PATH`, `MEMORY_STORE_BACKEND`, `MEMORY_EMBED_DIM` from
   the loaded config. The connection is typed over `surrealdb::engine::any::Any` and opened through
   `surrealdb::engine::any::connect(endpoint)`, so a single `Store` carries either an embedded engine
   or a remote endpoint — the engine is resolved from the endpoint scheme (FU-009): `MEMORY_STORE_REMOTE_URL`
   verbatim when set (e.g. `ws://host:8000`, `http(s)://…`; log a warning if `MEMORY_STORE_PATH` is also
   set — remote wins), otherwise `surrealkv://{MEMORY_STORE_PATH}` for the embedded default (in-process
   tests/ephemeral runs use `mem://`). A remote deployment therefore needs no code change. On any
   connection failure return `StoreError::Unavailable`. Log span `store.connect` with field `backend`.
   When the endpoint is remote **and** both `MEMORY_STORE_REMOTE_USER` and `MEMORY_STORE_REMOTE_PASS`
   are set, `connect` calls `db.signin(Root{ username, password })` after the connection opens, so a
   **secured** remote server is authenticated (FU-019); embedded and no-auth remote endpoints skip
   signin. `Config` enforces both-or-neither (exactly one set fails startup); the password is a
   redacted `Secret`. Per-operation `use_ns`/`use_db` (`ensure_and_use`) selects the tenant namespace
   on the authenticated session, so a root signin at connect is sufficient.
2. Migrations are applied lazily per tenant through the `Migrator` (see Data Model): every read/write
   method first calls `ensure_and_use(tenant)`, which runs `Migrator::migrate_up(tenant)` (idempotent —
   a no-op once the namespace is at the latest version) then selects the tenant namespace and the
   `recall` database. A SurrealDB timeout maps to `StoreError::Timeout`, any other engine error to
   `StoreError::Unavailable` (see `map_db_err`).
3. `ready()`: issue a trivial `RETURN 1` query against the live connection to confirm the engine
   answers; a failure maps via `map_db_err` (`StoreError::Timeout`/`StoreError::Unavailable`). The
   per-tenant vector-index dimension is fixed at migration time — the HNSW DDL embeds
   `MEMORY_EMBED_DIM` via the `Migrator` (the `<dim>` placeholder, SA-EMBED-01) — so it is not
   re-checked here; C8's `/readyz` compares `index_embed_dim()` against `MEMORY_EMBED_DIM`. The SQL
   storage has no vector index, so its `ready` also looks for a `mem_fact` row whose stored embedding
   is not `embed_dim × 4` bytes long and reports one as `StoreError::Internal` with a message beginning
   `stored embedding dimension`, the form every read of such an embedding reports. `StoreError` has no
   variant for it; the Memory Service recognises that prefix to answer `MEM_DIM_MISMATCH`.

**`ensure_tenant_namespace(tenant)`:**

4. Delegate to `Migrator::migrate_up(tenant)`. The Migrator validates the tenant id
   (`validate_tenant`), creates the namespace and `recall` database itself with `DEFINE NAMESPACE IF
   NOT EXISTS <tenant>; USE NS <tenant>; DEFINE DATABASE IF NOT EXISTS recall;` (DDL identifiers cannot
   be parameter-bound, so the validated tenant is interpolated), ensures the `schema_migrations`
   bookkeeping table, then applies every pending numbered migration's `DEFINE TABLE`/`DEFINE
   FIELD`/`DEFINE INDEX`/`DEFINE ANALYZER` statements (Data Model). All `DEFINE` statements use `IF NOT
   EXISTS`, so the call is idempotent. Engine errors map via `map_db_err`
   (`StoreError::Timeout`/`StoreError::Unavailable`). On the SQL storage every tenant shares one set
   of tables that the host applied once, so this call does nothing and returns `Ok(())`.

**`put_fact(f)` / `put_entity` / `put_relationship` / `put_source`:**

5. Validate the record against the domain rules in Shared Context (`entities.len() >= 1`; scores in
   `[0,1]`; `stability >= 0.0`; `valid_to >= valid_from`; `content` is a JSON object; string-length
   bounds). On failure return `StoreError::Validation` and do not write.
6. `USE NS <f.owner.tenant>` to select the namespace. If the configured/active namespace would differ
   from `f.owner.tenant`, return `StoreError::ScopeForbidden` (a record may not be written outside its
   own tenant namespace).
7. Execute a parameterised `UPSERT $id CONTENT $rec` inside `UPSERT_OWNED`, binding the record id as
   `$id` (a record-id value built from the "table:key" id, split on the first `:`) and the native row
   object as `$rec` — never string-interpolated. `UPSERT_OWNED` is one transaction: it reads the stored
   `owner`, throws when that row belongs to another user (mapped to `StoreError::ScopeForbidden`), runs
   the `UPSERT`, and writes the stored `owner` back. The SQL storage's equivalent is `INSERT … ON
   CONFLICT (id) DO UPDATE … WHERE` the stored tenant and owner user equal the record's; no row changed
   → `StoreError::ScopeForbidden`. The row is built by `convert::*_to_object`; for `put_fact` it also
   carries the derived `content_text` BM25 projection (`convert::content_to_text`), the `embedding`
   field (the fact-content vector supplied by C4, written as `NONE` on first write) and an
   `embedding_model` version string, both filled later by C7 re-embedding via `set_fact_embedding`.
   `append_audit` uses `CREATE $id CONTENT $rec` (append-only). On a store error return
   `StoreError::Unavailable`; on per-statement timeout `StoreError::Timeout`. Logged at `store.put`.

**`get_fact(ctx, id)` / `get_entity` / `get_relationship` / `get_source`:**

8. `ensure_and_use(ctx.tenant)` (select + lazily migrate the namespace). Run a parameterised
   `SELECT * FROM $id WHERE <read-filter>` binding the record id as `$id` and the read-filter
   parameters `$cuser` (`ctx.user`) and `$cteams` (`ctx.teams`) — never request-body input. `get_fact`
   uses the maintenance-aware `fact_read_filter(ctx)` (the whole tenant for a maintenance scope, else
   `fact_filter_clause()`); `get_entity` and `get_relationship` apply the Entity and Relationship
   filters of Shared Context; `get_source` uses `novis_filter_clause()`.
   If the record exists but the predicate excludes it, return `Ok(None)` (a caller learns nothing about
   records it cannot read; this is the read-side equivalent of not-found). If the record does not
   exist, return `Ok(None)`. The decoded row drops the persistence-only `embedding`, `embedding_model`,
   and `content_text` columns before deserialising into the domain `Fact`.

**`recall(ctx, q)` (multi-signal stage-1, NFR-P3 / SA-LAT-01):**

9. `USE NS <ctx.tenant>`. Run up to three parameterised sub-queries, each constrained by the read-filter
   predicate and the `q.filters` (`memory_class`, `visibility`, `entity`, and `valid_at` — when
   `valid_at` is set, restrict to facts whose `valid_from <= valid_at AND (valid_to IS NONE OR valid_to
   > valid_at)`; when unset, restrict to currently-valid facts `valid_to IS NONE`):
   - **Vector signal** (skipped if `q.query_vector` is empty): SurrealDB HNSW approximate-nearest-
     neighbour search through the `<|K,EF|>` KNN operator, which drives the `fact_hnsw` index — an
     O(log N) index walk, not the prior O(N) `vector::similarity::cosine(...) ORDER BY` brute-force
     scan (RISK-008, resolved). The query is `SELECT meta::id(id) AS fid, vector::distance::knn() AS
     dist FROM fact WHERE embedding <|knn_k,ef|> $qv AND (<read-filter>) AND <validity><meta> ORDER BY
     dist ASC LIMIT $k` with `$qv = q.query_vector`, `$k = q.stage1_k`. `vector::distance::knn()`
     reports each match's cosine distance computed during the index walk. `K` (`knn_k`) and `EF`
     (`ef`) are operator syntax and cannot be parameter-bound, so they are formatted as validated
     integers — never caller input: `knn_k = (4 × stage1_k).clamp(stage1_k, 1024)` (a 4× over-fetch so
     the KNN-then-filter ordering rarely starves the final set), `ef = knn_k.max(64)`. The scope
     read-filter, validity, and metadata predicates are AND-ed on top of the KNN operator
     (KNN-then-filter), the result ordered by ascending distance and capped to `stage1_k`. Cosine
     similarity is recovered as `1 - dist` and clamped to `[0,1]`. The ANN portion must complete in
     ≤ 50 ms (NFR-P3); on per-statement timeout return `StoreError::Timeout`.
   - **Keyword signal** (skipped if `q.keyword_terms` is empty): BM25 full-text search over the
     derived `content_text` projection using the `@0@` full-text match operator and the `search::score(0)`
     relevance function against the `fact_bm25` index, `LIMIT $k`. The query is `SELECT meta::id(id) AS
     fid, search::score(0) AS s FROM fact WHERE content_text @0@ $terms AND (<read-filter>) AND
     <validity><meta> ORDER BY s DESC LIMIT $k`, with `$terms` the caller's `keyword_terms` joined by
     spaces.
   - **Graph signal** (run when `q.filters.entity` is set): select currently-valid, read-filtered
     facts whose `entities` array contains the given entity id, served by the `fact_ent` index —
     `SELECT meta::id(id) AS fid FROM fact WITH INDEX fact_ent WHERE $entity IN entities AND
     (<read-filter>) AND <validity> LIMIT $k`. (`entities` holds "table:key" entity id strings.
     Multi-hop `relationship`-edge traversal beyond this direct-membership query is not implemented in
     this method — see Gaps.)

     Two conditions make this leg index-served, and neither is implied by the query text (RISK-018):

     1. `fact_ent` must be defined over `entities.*`, not `entities`. An index over the whole array
        value cannot serve element matching in SurrealDB 3.1.5 — the planner ignores it, and an
        explicit hint against it degrades to a full table scan. Migration `0002` corrects the `0001`
        definition.
     2. The `WITH INDEX fact_ent` hint must be present. With a correct index but no hint the planner
        still selects `fact_valid`, an index on `valid_to` that matches every live fact, because it has
        no selectivity statistics and the bi-temporal predicate is what flips the choice.

     The hint is sound because `$entity IN entities` is a *conjunct*: the index yields a superset and
     the read-filter and validity predicates are applied above it. The same hint would be **wrong** on
     the read-filter itself, which is a disjunction — forcing one index across an `OR` drops the other
     arms rather than narrowing the scan. Both conditions fail silently (correct rows, full scan), so
     they are pinned by a plan assertion rather than by a result assertion.
10. Merge the three result sets, keyed by the bare fact id returned by each sub-query's `meta::id(id)
    AS fid` projection, into `Candidate`s, populating `semantic_score`, `keyword_score`, `graph_score`
    (0.0 for any signal that did not surface that fact; the graph signal contributes a fixed `1.0`).
    Normalise BM25 scores to `[0,1]` by dividing by the maximum `search::score(0)` in the keyword
    result set (0.0 when the set is empty). Resolve each surviving id (prefixed back to `fact:<fid>`)
    to its full `Fact` by calling `get_fact(ctx, id)`, which re-applies the read filter. Return up to
    `q.stage1_k` candidates. No fusion or rerank is done here — that is C6. Logged at `store.recall`
    with `candidate_count`.

**`end_validity(ctx, id, at)` / `end_relationship_validity`:**

11. `USE NS <ctx.tenant>`. Confirm read access via the read filter; if excluded or absent return
    `StoreError::NotFound`. Apply `owner_gate` (a caller who can read the record and does not own it
    gets `StoreError::ScopeForbidden`). Run a parameterised `UPDATE <table>:⟨id⟩ SET valid_to = $at WHERE valid_to
    IS NONE`. Setting `valid_to` retires the record without deleting it (ADR-002). If the record was
    already retired (`valid_to` already set), the update is a no-op and returns `Ok(())` (idempotent).
    Logged at `store.end_validity`.

**`supersede(ctx, old_id, new_id, at)`:**

12. `USE NS <ctx.tenant>`. Confirm read access to both ids; if either is excluded or absent return
    `StoreError::NotFound`. Apply `owner_gate` to both. In one parameterised transaction: `UPDATE fact:⟨old⟩ SET valid_to = $at,
    superseded_by = $new` and `UPDATE fact:⟨new⟩ SET supersedes = $old`. Neither fact is deleted
    (ADR-002). On store failure return `StoreError::Unavailable`. Logged at `store.supersede`.

**`hard_delete(ctx, id)` (SA-DELETE-01):**

13. `ensure_and_use(ctx.tenant)`. Confirm `forget` access path via `get_fact(ctx, id)` (the operation
    is authorised by C8; the store enforces tenant + read-filter scoping); if excluded or absent return
    `StoreError::NotFound`. Apply `owner_gate`. Collect every consolidated-insight fact whose `derived_from` contains this
    id with a parameterised `SELECT * FROM fact WHERE $base IN derived_from AND (<read-filter>)`, served
    by the `fact_derived` index (added in migration `0003`; without it this query scans the whole `fact`
    table on an erasure path, where the work should be proportional to the lineage rather than to the
    namespace). No `WITH INDEX` hint is needed here, unlike the graph leg — this query carries no
    bi-temporal predicate, so there is no `fact_valid` for the planner to prefer. Adding one later would
    flip the choice, which is why the plan is asserted rather than assumed. This
    derived-collection query uses the maintenance-aware `fact_read_filter(ctx)` so a maintenance scope
    reaches user-private and team-shared derived insights (RISK-009). Keep only the insights the caller
    owns (`owner_view`; every one under a maintenance scope): a teammate's summary of this fact stays,
    with an ended origin (SA-AUTHZ-07). Folder and team erasure are not owner-bound and do remove the
    team's summaries (steps 20c, 20s). Build the removal set — the fact
    itself plus each kept insight — and count embedding vectors removed (the fact's own embedding
    plus any derived-summary embeddings, each probed via a `SELECT embedding FROM $id` non-null check).
14. Remove the jobs that wrote the removal set's facts, with their dead letters and links
    (`drop_jobs_of`, step 13″). Delete all collected records in one parameterised `DELETE $ids` (the ids
    bound as a list of record-id values). Compute `digest` as the SHA-256 hex of the newline-joined, sorted removed ids
    and build `DeletionProof { deleted_at: now, record_id: id, derived_removed: [...],
    embeddings_removed: N, digest }`. After deleting, re-read each collected id via `get_fact`; if any
    is still present the removal is partial — return `StoreError::PartialDelete { removed, expected }`
    (`removed = expected - still_present`) and never report the delete complete (SA-DELETE-01). The SQL
    storage runs this re-read before its commit, so a partial delete leaves every row as it was. Once
    the delete is complete, call `sweep_summaries(ctx, removed ids)` (step 20r). The record set includes
    the entities and sources of step 13‴. Logged
    at `store.hard_delete` with `derived_removed_count`, `embeddings_removed`.

**`find_entity_by_name(ctx, name)` (entity-resolution support for C4):**

15. `USE NS <ctx.tenant>`. Run a parameterised `SELECT * FROM entity WHERE (canonical_name = $name OR
    $name IN aliases OR (owner.user = $cuser AND $name IN private_aliases)) AND <read-filter>`, the
    Entity filter of Shared Context. A private alias matches for its owner alone. Return all matches
    (the caller resolves ties) as each is seen by the caller. Logged at `store.find_entity`.

**`append_audit(e)` (SA-AUDIT-01):**

16. `USE NS <e.tenant>`. Run a parameterised `CREATE audit_log:⟨id⟩ CONTENT $entry`. The `audit_log`
    table is append-only (Data Model permissions forbid `UPDATE`/`DELETE`); it stores subject,
    operation, scope, outcome, `token_jti`, correlation id, the optional calling `client` identity, the
    optional `fact_class` **label** attempted, and timestamp — never the token, never fact content.
    Called synchronously by C9 before the response is returned. On store failure return
    `StoreError::Unavailable`. Logged at `store.audit`. `client` and `fact_class` are the two fields
    added by migration `0005`; they carry an identity and a label the caller supplied, never any part
    of `content`, so the absolute no-content rule (SA-AUDIT-01) is unchanged by their addition.

**Maintenance surface (consumed by C7; each uses `ctx.tenant` to select the namespace):**

17. `scan_decay_candidates(ctx, salience_floor, limit)`: `USE NS <ctx.tenant>`; parameterised `SELECT *
    FROM fact WHERE valid_to IS NONE AND salience < $floor ORDER BY last_recalled_at ASC NUMERIC LIMIT
    $limit` (rows with a NULL `last_recalled_at` order first via `ingested_at`). This is a coarse
    prefilter; C7 computes the Ebbinghaus retrievability `R = exp(-Δt / (stability · k))` and decides
    pruning (ADR-006). (SA-DECAY-01.)
18. `scan_contradiction_candidates(ctx, limit)`: `USE NS <ctx.tenant>`; parameterised query returning
    pairs of currently-valid facts that share at least one entity id, capped at `limit`.
19. `scan_reembed_candidates(ctx, current_model_version, limit)`: `USE NS <ctx.tenant>`; parameterised
    `SELECT * FROM fact WHERE valid_to IS NONE AND (embedding_model != $current OR embedding IS NONE)
    LIMIT $limit`.
19a. `scan_recent_episodes(ctx, since, limit)`: `USE NS <ctx.tenant>`; parameterised `SELECT * FROM fact
    WHERE valid_to IS NONE AND memory_class = 'episodic' AND ingested_at >= $since LIMIT $limit`.
19b. `update_fact_maintenance_fields(ctx, f)`: `USE NS <ctx.tenant>`; confirm read access to `f.id`
    (else `StoreError::NotFound`); parameterised `UPDATE fact:⟨f.id⟩ SET confidence = $c, salience = $s,
    stability = $st, last_recalled_at = $lr, supersedes = $sup, superseded_by = $supby` — content,
    owner, validity, and class are never touched by this method. Logged at `store.update_maint`.
19c. `set_fact_embedding(ctx, fact_id, vector, model_version)`: `USE NS <ctx.tenant>`; assert
    `vector.len() == MEMORY_EMBED_DIM` (else `StoreError::Validation`); parameterised `UPDATE
    fact:⟨fact_id⟩ SET embedding = $v, embedding_model = $m`. Logged at `store.set_embedding`.
19d. `merge_entities(ctx, keep_id, merge_id)`: `ensure_and_use(ctx.tenant)`; confirm read access to
    both via `get_entity` (else `StoreError::NotFound`); apply `owner_gate` to both. Refuse a merge that
    would publish `keep`'s name (`refuse_name_exposure`): when `keep` is named by a private fact and no
    shared one, and `merge` is named by a shared one, return `StoreError::ScopeForbidden`, because the
    moved shared facts would show `keep`'s canonical name to their readers. The merge the other way is
    allowed: `fold_aliases` adds `merge`'s names to `keep.private_aliases` when no shared fact names
    `merge`, and keeps a private alias of `merge` private. In one `BEGIN; … COMMIT;` transaction repoint
    every `fact.entities` membership and every `relationship.from_ent`/`to_ent` string reference from
    `merge_id` to `keep_id` (the relationship endpoints are stored as the `from_ent`/`to_ent` string
    fields, not `from`/`to` — see Data Model), fold `merge_id`'s aliases + canonical_name into
    `keep_id.aliases`, each with its folders: `merge`'s `alias_folders` records, and for its canonical
    name also `merge`'s own `owner.team` and `collection`, so a name folded in goes when its last folder
    is erased. Then `DELETE $merge`. The repoint changes references only, so another user's
    facts and links follow it. Logged at `store.merge_entities`.
19e. `list_tenants()`: `INFO FOR ROOT` to enumerate defined namespaces (the `namespaces` object's
    keys); return the tenant ids. Admin-only — takes no `ctx`; callers (C2 reaper, C7 scheduler)
    iterate the result and build a per-tenant `ScopeContext`. Logged at `store.list_tenants`. The SQL
    storage has no namespaces: it returns the distinct tenants of its tenant-carrying tables.

**`erase_tenant(tenant)` (ADR-018 per-tenant erasure):**

20. Validate `tenant` (`validate_tenant`), then `ensure_provisioned(tenant)` so every table named
    below exists — erasing a tenant that was never written is a zero-count proof, not an error.
    Resolve the physical namespace once through `placement::resolve`; the whole erasure runs there.
    The SQL storage shares one set of tables among tenants and names `mem` as the proof's namespace.
20a. For each `(table, tenant_field)` pair in the erasure set below, issue one namespace-scoped batch:
    `SELECT count() FROM <table> WHERE <tenant_field> = $t GROUP ALL` (the count that goes in the
    proof), then `DELETE <table> WHERE <tenant_field> = $t`. `<table>` and `<tenant_field>` are
    compile-time constants, never caller input; `$t` is bound (sql-safety layer rule). The erasure set
    is every table in the namespace that carries a tenant, and it must stay exhaustive — a table added
    to the schema without an entry here leaves that tenant's rows behind:

    | Table | Tenant field | Owning component |
    |---|---|---|
    | `fact` | `owner.tenant` | C1 |
    | `entity` | `owner.tenant` | C1 |
    | `relationship` | `owner.tenant` | C1 |
    | `source` | `owner.tenant` | C1 |
    | `audit_log` | `scope.tenant` | C1 |
    | `grant` | `grantor.tenant` | Consent Grants |
    | `work_job` | `scope.tenant` | C2 |
    | `dead_letter` | `scope.tenant` | C2 |
    | `quarantine` | `owner.tenant` | C4 |
    | `maintenance_state` | `tenant` | C7 |
    | `idempotency_record` | `tenant` | C8 |
    | `job_fact` | `scope.tenant` | C1 |

    `relationship_fact` carries no tenant: its rows go first, with the tenant's relationships and
    facts, and are not counted in the proof. On the SQL storage the set is the same tables with the
    `mem_` prefix plus `mem_entity_name`, in the `tenant` column, and the junction tables go with
    their fact. `schema_migrations` is deliberately absent: it records the namespace's schema version, not any
    tenant's data, and under co-residency it is shared. `audit_log`'s `PERMISSIONS … FOR delete NONE`
    does not block this delete — table permissions bind record-level sessions and C1 holds a root
    connection — and the trail being erasable with its tenant is the stated regulatory position
    (06-regulatory, auditability row).
20b. **Verification.** Re-count every table in the erasure set on the same predicate. Any non-zero
    remainder returns `StoreError::PartialDelete { removed, expected }`, where `expected` is the
    pre-delete total and `removed` is `expected` minus the remainder: an incomplete erasure never
    reports success (the SA-DELETE-01 rule, applied to the tenant path).
20c. Return `TenantErasureProof` with the per-table counts, their total, the resolved namespace, and
    the digest over them. Logged at `store.erase_tenant` with `total_removed` and `tenant_present` —
    never the tenant id, per the Observability rule below.

**`drop_tenant_namespace(tenant)` (ADR-011 optimisation — single-tenant namespaces only):**

20d. Validate `tenant`, then check the namespace is exclusive to it: the active memory profile must
    declare that its namespaces hold one tenant each, **and** `placement::resolve` must map `tenant`
    onto a namespace of the same name. Both must hold; either failing returns
    `StoreError::NamespaceNotExclusive { tenant, namespace }` and nothing is dropped. The two checks
    are deliberately independent — a later topology change that shares namespaces trips the second
    even if it forgets to update the first, and this operation is irreversible, so a redundant guard
    costs less than the failure it prevents. The SQL storage's tables hold every tenant, so it always
    returns `StoreError::NamespaceNotExclusive`, with `mem` as the namespace.
20e. `REMOVE NAMESPACE IF EXISTS <tenant>` (the tenant id validated by `validate_tenant` before
    interpolation, since DDL identifiers cannot be parameter-bound) — removes every Fact, Entity,
    Relationship, Source, audit entry, and derived artefact for the tenant in one structural
    operation. It returns no proof: it removes the tables a proof would have counted, so there is
    nothing left to verify against. A caller that needs a proof calls `erase_tenant`. This is a
    destructive operation; see Data Model rollback considerations. Logged at `store.drop_ns`.

Every external store call carries a per-statement timeout (default 2000 ms, the read-path store budget
within SA-LAT-01); a breach returns `StoreError::Timeout`. All logs are structured and include
`correlation_id` from `ctx` where a `ctx` is present.

#### Data Model

SurrealDB DDL applied per tenant through the `Migrator` (SA-MIG-01). One namespace per tenant
(ADR-011); one database `recall` inside each. The five originally-numbered migrations (init, queue,
quarantine, maintenance, idempotency) were squashed into a single initial migration
`migrations/0001_init.{up,down}.surql` while no database had shipped (greenfield), so
`version: 1, slug: "init"` is the first entry the `Migrator` registers; later migrations,
up to `0009_job_fact` (`LATEST = 9`), follow it. This component owns the C1 portion of that one migration — the `fact`,
`entity`, `relationship`, `source`, and `audit_log` tables; the same file also carries the C2 queue
(`work_job`/`dead_letter`), C4 `quarantine`, C7 `maintenance_state`, and C8 `idempotency_record`
tables owned by their components. Tables `fact`, `entity`, `relationship`, `source` are
**schemaless** with typed `DEFINE FIELD` constraints and tighten further as the model stabilises
(SA-MIG-01); `audit_log` is **schemafull and append-only** because its shape is fixed and
integrity-critical. All `DEFINE` statements are idempotent (`IF NOT EXISTS`) so the migration is safe
to repeat.

The namespace and database DDL is **not** in the `.surql` file: the `Migrator` selects the namespace
itself before running the migration body — `DEFINE NAMESPACE IF NOT EXISTS <tenant>; USE NS <tenant>;
DEFINE DATABASE IF NOT EXISTS recall;` then `use_ns(tenant).use_db("memory")` — and tracks applied
versions in a `schema_migrations` table created **outside** the numbered set (so version 0 is readable
on a fresh namespace). The HNSW `DIMENSION` is the literal placeholder `<dim>` in the `.surql` file,
substituted by the Migrator (`render_up`) from `MEMORY_EMBED_DIM` before execution (SA-EMBED-01).

SurrealDB 3.x adaptations baked into the as-built DDL (FU-008): BM25 uses `FULLTEXT ANALYZER … BM25`
over a derived `content_text` projection (the 2.x `SEARCH ANALYZER` form is rejected); schemafull
tables carrying arbitrary nested JSON use `TYPE object FLEXIBLE`; and domain ids are stored as
"table:key" strings throughout (see `src/store/surreal/convert.rs`), so link-shaped fields are `string` /
`array<string>` / `option<string>`, not native record links.

```surql
-- src/store/surreal/migrate.rs SCHEMA_MIGRATIONS_DDL — created outside the numbered set so version 0 is readable
DEFINE TABLE IF NOT EXISTS schema_migrations SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS version    ON schema_migrations TYPE int;
DEFINE FIELD IF NOT EXISTS applied_at ON schema_migrations TYPE datetime;
DEFINE INDEX IF NOT EXISTS sm_version ON schema_migrations FIELDS version UNIQUE;
```

```surql
-- migration 0001_init.up.surql (C1 portion) — applied within each tenant namespace/database.
-- The Migrator selects the namespace (USE NS <tenant>; USE DB memory) before running these statements,
-- so the file body contains only table/field/index DDL (no DEFINE NAMESPACE/DATABASE).

-- fact: schemaless body, indexed for the three retrieval signals
DEFINE TABLE IF NOT EXISTS fact SCHEMALESS;
DEFINE FIELD IF NOT EXISTS owner       ON fact TYPE object;
DEFINE FIELD IF NOT EXISTS valid_from  ON fact TYPE datetime;
DEFINE FIELD IF NOT EXISTS valid_to    ON fact TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS ingested_at ON fact TYPE datetime;
DEFINE FIELD IF NOT EXISTS confidence  ON fact TYPE float ASSERT $value >= 0.0 AND $value <= 1.0;
DEFINE FIELD IF NOT EXISTS salience    ON fact TYPE float ASSERT $value >= 0.0 AND $value <= 1.0;
DEFINE FIELD IF NOT EXISTS stability   ON fact TYPE float ASSERT $value >= 0.0;
DEFINE FIELD IF NOT EXISTS visibility  ON fact TYPE string
    ASSERT $value IN ['user-private','team-shared','tenant-shared'];
DEFINE FIELD IF NOT EXISTS memory_class ON fact TYPE string
    ASSERT $value IN ['episodic','semantic','consolidated'];
DEFINE FIELD IF NOT EXISTS pii_review      ON fact TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS embedding       ON fact TYPE option<array<float>>;
DEFINE FIELD IF NOT EXISTS embedding_model ON fact TYPE option<string>;
-- BM25 keyword analyzer + full-text index over the derived content_text projection (3.x syntax)
DEFINE ANALYZER IF NOT EXISTS memory_text TOKENIZERS class FILTERS lowercase, ascii;
DEFINE INDEX IF NOT EXISTS fact_bm25 ON fact FIELDS content_text FULLTEXT ANALYZER memory_text BM25;
-- HNSW vector index; DIMENSION is the <dim> placeholder, substituted from MEMORY_EMBED_DIM (SA-EMBED-01)
DEFINE INDEX IF NOT EXISTS fact_hnsw ON fact FIELDS embedding HNSW DIMENSION <dim> DIST COSINE;
-- secondary indexes for scope filtering, the graph leg, and bi-temporal scans
DEFINE INDEX IF NOT EXISTS fact_owner_user ON fact FIELDS owner.user;
DEFINE INDEX IF NOT EXISTS fact_owner_team ON fact FIELDS owner.team;
DEFINE INDEX IF NOT EXISTS fact_valid      ON fact FIELDS valid_to;
DEFINE INDEX IF NOT EXISTS fact_ent        ON fact FIELDS entities.*;  -- graph-leg membership index
DEFINE INDEX IF NOT EXISTS fact_derived    ON fact FIELDS derived_from.*;  -- hard_delete lineage index
-- `entities.*` indexes each element. Over `entities` (the 0001 definition, corrected by migration
-- 0002) the index covers the array as a whole value and cannot serve `$entity IN entities` at all.

DEFINE TABLE IF NOT EXISTS entity SCHEMALESS;
DEFINE FIELD IF NOT EXISTS canonical_name ON entity TYPE string
    ASSERT string::len($value) > 0 AND string::len($value) <= 512;
DEFINE FIELD IF NOT EXISTS aliases        ON entity TYPE array<string>;
DEFINE FIELD IF NOT EXISTS owner          ON entity TYPE object;
DEFINE INDEX IF NOT EXISTS entity_name    ON entity FIELDS canonical_name;
DEFINE INDEX IF NOT EXISTS entity_owner   ON entity FIELDS owner.user, owner.team;

DEFINE TABLE IF NOT EXISTS relationship SCHEMALESS;
DEFINE FIELD IF NOT EXISTS kind       ON relationship TYPE string
    ASSERT string::len($value) > 0 AND string::len($value) <= 128;
-- endpoints stored as plain "table:key" strings in from_ent/to_ent; the domain `from`/`to` clash with
-- SurrealQL reserved words, and ids are strings throughout, so these are TYPE string not record links
DEFINE FIELD IF NOT EXISTS from_ent   ON relationship TYPE string;
DEFINE FIELD IF NOT EXISTS to_ent     ON relationship TYPE string;
DEFINE FIELD IF NOT EXISTS valid_from ON relationship TYPE datetime;
DEFINE FIELD IF NOT EXISTS valid_to   ON relationship TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS confidence ON relationship TYPE float ASSERT $value >= 0.0 AND $value <= 1.0;
DEFINE FIELD IF NOT EXISTS owner      ON relationship TYPE object;
DEFINE INDEX IF NOT EXISTS rel_from   ON relationship FIELDS from_ent;
DEFINE INDEX IF NOT EXISTS rel_to     ON relationship FIELDS to_ent;

DEFINE TABLE IF NOT EXISTS source SCHEMALESS;
DEFINE FIELD IF NOT EXISTS origin_ref           ON source TYPE string;
DEFINE FIELD IF NOT EXISTS modification_marker  ON source TYPE option<string>;
DEFINE FIELD IF NOT EXISTS trust_signal         ON source TYPE float ASSERT $value >= 0.0 AND $value <= 1.0;
DEFINE FIELD IF NOT EXISTS owner                ON source TYPE object;

-- audit_log: schemafull + append-only from the first migration (SA-AUDIT-01)
DEFINE TABLE IF NOT EXISTS audit_log SCHEMAFULL
    PERMISSIONS FOR create FULL FOR select FULL FOR update NONE FOR delete NONE;
DEFINE FIELD IF NOT EXISTS subject        ON audit_log TYPE string;
DEFINE FIELD IF NOT EXISTS operation      ON audit_log TYPE string;
-- `scope` is TYPE object FLEXIBLE so the audit ScopeRef sub-keys (tenant/team/user) write on 3.x (FU-016)
DEFINE FIELD IF NOT EXISTS scope          ON audit_log TYPE object FLEXIBLE;
DEFINE FIELD IF NOT EXISTS outcome        ON audit_log TYPE string;
DEFINE FIELD IF NOT EXISTS token_jti      ON audit_log TYPE string;
DEFINE FIELD IF NOT EXISTS correlation_id ON audit_log TYPE string;
DEFINE FIELD IF NOT EXISTS at             ON audit_log TYPE datetime;
DEFINE INDEX IF NOT EXISTS audit_at ON audit_log FIELDS at;

-- migration 0005_audit_authority.up.surql — additive, forward-only (SA-AUTHZ-04).
-- `audit_log` is SCHEMAFULL, so a write carrying an undeclared key is rejected outright; these two
-- DEFINE FIELDs are what let the refusal record name who was refused and what class they attempted.
DEFINE FIELD IF NOT EXISTS client     ON audit_log TYPE option<string>;
DEFINE FIELD IF NOT EXISTS fact_class ON audit_log TYPE option<string>;

-- `fact.fact_class` (SA-AUTHZ-09) needs **no DDL**: `fact` is SCHEMALESS, so `fact_to_object` simply
-- writes one more key. Only the SCHEMAFULL tables need a migration for a new column — `audit_log` did
-- (0005) and `quarantine` does (0006, C4-owned). Stating that here so a reader does not go looking
-- for a `fact` migration that deliberately does not exist.

-- The same 0001_init.up.surql also defines the C2 (work_job, dead_letter), C4 (quarantine),
-- C7 (maintenance_state), and C8 (idempotency_record) tables owned by those components.
```

The `content_text` column is a derived flat-keyword projection of the structured `content` object —
every scalar leaf concatenated with spaces, built by `convert::content_to_text` on each `put_fact` and
stored alongside the intact `content` object. The `fact_bm25` index is defined over `content_text`, and
the keyword recall sub-query matches `content_text @0@ $terms`. `content_text` (with `embedding` and
`embedding_model`) is a persistence-only column dropped on read before deserialising the domain `Fact`.

```surql
-- migration 0001_init.down.surql — destructive reversal (the C1 portion shown)
REMOVE INDEX fact_hnsw ON fact;       REMOVE INDEX fact_bm25 ON fact;
REMOVE INDEX fact_owner_user ON fact; REMOVE INDEX fact_owner_team ON fact;
REMOVE INDEX fact_valid ON fact;      REMOVE INDEX fact_ent ON fact;
REMOVE INDEX entity_name ON entity;   REMOVE INDEX entity_owner ON entity;
REMOVE INDEX rel_from ON relationship; REMOVE INDEX rel_to ON relationship;
REMOVE INDEX audit_at ON audit_log;
REMOVE TABLE audit_log; REMOVE TABLE source; REMOVE TABLE relationship; REMOVE TABLE entity; REMOVE TABLE fact;
-- (the down file also drops the C2/C4/C7/C8 tables and indexes from the same squashed migration)
```

**Rollback considerations (sql-safety layer):**

- `0006_quarantine_fact_class` (C4-owned, defined in `components/write-pipeline.md`) is **additive
  and non-destructive**: one `DEFINE FIELD IF NOT EXISTS … TYPE option<string>` on the SCHEMAFULL
  `quarantine` table. Its down path removes the column, discarding the record of what each held write
  claimed to be; the quarantine rows themselves survive. `fact.fact_class` needs no migration at all —
  `fact` is SCHEMALESS.
- `0005_audit_authority` is **additive and non-destructive**: two `DEFINE FIELD IF NOT EXISTS … TYPE
  option<string>` statements against the existing `audit_log` table. It backfills nothing, rewrites no
  row, and collapses no bi-temporal history — `audit_log` is append-only and holds none. Rows written
  before it read back with `client` and `fact_class` as `None`, which is indistinguishable from a call
  that carried neither, so the trail stays readable across the boundary without a migration of data.
- Its down path is `REMOVE FIELD client ON audit_log` + `REMOVE FIELD fact_class ON audit_log`. That
  **discards the two columns and the values in them** on any row written since the up migration ran —
  the audit rows themselves survive, but the record of which service acted and which class was
  refused does not. Reversing this migration on a deployment that has relied on the class control
  therefore destroys exactly the evidence the control was added to produce. Take a copy of `audit_log`
  before reversing it, or leave the fields in place and roll back only the code.
- The single `0001_init` down path `REMOVE TABLE`/`REMOVE INDEX` is **destructive** — it discards every
  fact, entity, relationship, source, and audit record in the namespace (and the C2/C4/C7/C8 tables in
  the same squashed migration) and drops the vector/keyword indexes. It is intended only to unwind a
  failed initial migration on an empty store. `Migrator::migrate_down` logs the intent but does not
  guard population — that decision is the caller's (an explicit user action). It must never run against
  a populated production namespace; applies to a shared store are a user action with a `dry_run` first
  (SA-MIG-01).
- `erase_tenant` is **irreversible and intentionally destructive** — it is the ADR-018 per-tenant
  erasure mechanism, and there is no down path: the data is gone by design. What it destroys is
  bounded by the predicate, so it is safe to run in a namespace holding other tenants; a row it
  removes always carried the erased tenant in the field named in the erasure set.
- `drop_tenant_namespace` (`REMOVE NAMESPACE IF EXISTS`) is **irreversible, intentionally destructive,
  and unbounded by any predicate** — it removes the namespace and therefore everything in it. That is
  equivalent to erasing one tenant only where the namespace holds exactly one, which is why the guard
  in step 20d refuses it anywhere else (ADR-018). Before that guard existed, the operation would have
  erased every co-resident tenant on the code path a data subject invokes to exercise a right
  (RISK-021).
- Any future schemaless→schemafull tightening for `fact`/`entity`/`relationship`/`source` would be a
  **later migration** (version 2+) that adds `ASSERT`/type constraints; tightening a populated table can
  reject rows that violate a new constraint, so such a migration must include a forward
  data-validation scan and a down path that widens the field back to schemaless (non-destructive).
  Changing the HNSW `DIMENSION` is destructive (it drops and rebuilds the index and requires
  re-embedding all facts via C7) and is out of scope for `0001_init`; the `<dim>` placeholder fixes the
  dimension from `MEMORY_EMBED_DIM` at first migration only.

#### Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Record fails domain validation (empty `entities`, score out of `[0,1]`, `valid_to < valid_from`, non-object `content`, name length) | 400 | VAL_INVALID_BODY | `{"error":{"code":"VAL_INVALID_BODY","message":"record failed validation","correlation_id":"<uuid>"}}` |
| Score field outside `[0.0,1.0]` (`confidence`/`salience`/`trust_signal`) | 400 | VAL_OUT_OF_RANGE | `{"error":{"code":"VAL_OUT_OF_RANGE","message":"score out of range","correlation_id":"<uuid>"}}` |
| `get_fact`/`end_validity`/`hard_delete`/`supersede` target id absent or excluded by the read filter | 404 | NOT_FOUND | `{"error":{"code":"NOT_FOUND","message":"record not found","correlation_id":"<uuid>"}}` |
| `put_*` record `owner.tenant` differs from the active namespace (cross-tenant write attempt), or its id already belongs to another user | 403 | SCOPE_FORBIDDEN | `{"error":{"code":"SCOPE_FORBIDDEN","message":"record outside caller tenant","correlation_id":"<uuid>"}}` |
| The caller can read the target of `end_validity`, `supersede`, `hard_delete`, `set_pii_review`, `keep_fact`, `end_relationship_validity`, `delete_relationship`, `erase_entity` or `merge_entities` and does not own it; a merge whose plan deletes another user's relationship (step 19d′); a merge that would publish `keep`'s private name (step 19d) | 403 | SCOPE_FORBIDDEN | `{"error":{"code":"SCOPE_FORBIDDEN","message":"forbidden","correlation_id":"<uuid>"}}` |
| Store connection lost / engine not reachable | 503 | STORE_UNAVAILABLE | `{"error":{"code":"STORE_UNAVAILABLE","message":"memory store unavailable","correlation_id":"<uuid>"}}` |
| Per-statement timeout (read-path store op exceeds budget; ANN > 50 ms or query > 2000 ms) | 504 | STORE_TIMEOUT | `{"error":{"code":"STORE_TIMEOUT","message":"memory store timed out","correlation_id":"<uuid>"}}` |
| A recall with no `filters.collections` has more than 32 collections in play (SA-COLL-02) | 400 | VAL_TOO_MANY_COLLECTIONS | `{"error":{"code":"VAL_TOO_MANY_COLLECTIONS","message":"more than 32 collections in play: name the ones to search in filters.collections","correlation_id":"<uuid>"}}` |
| `hard_delete` verification finds a collected record still present (partial delete) | 500 | INTERNAL | `{"error":{"code":"INTERNAL","message":"deletion incomplete","correlation_id":"<uuid>"}}` |
| `erase_tenant` verification finds rows still carrying the tenant after the delete (partial erasure) | 500 | INTERNAL | `{"error":{"code":"INTERNAL","message":"deletion incomplete","correlation_id":"<uuid>"}}` |
| `drop_tenant_namespace` called where the namespace may hold tenants other than the named one | 403 | SCOPE_FORBIDDEN | `{"error":{"code":"SCOPE_FORBIDDEN","message":"record outside caller tenant","correlation_id":"<uuid>"}}` |

`StoreError` → `code` mapping is owned by the Phase 4 Error Handling spec; this table states the
intended mapping for every failure mode the Internal Logic references. `StoreError::Validation` carries
one detail string and no kind: a detail containing `range` maps to `VAL_OUT_OF_RANGE`, every other
detail to `VAL_INVALID_BODY`, so a score-range failure says `… out of range`;
`StoreError::PartialDelete` maps to `INTERNAL` because a partial delete is an internal invariant
violation, never a client error; `StoreError::NamespaceNotExclusive` maps to `SCOPE_FORBIDDEN` because
the refusal is exactly a scope violation — the operation would have reached outside the named tenant.

#### Acceptance Criteria (Gherkin)

```gherkin
Feature: Memory Store

  Scenario: Happy path — put a fact then recall it by vector + keyword
    Given a provisioned tenant namespace "acme"
    And a fact owned by {tenant "acme", team "alpha", user "u-sarah"} with visibility "team-shared" is stored via put_fact
    When recall is called with ctx {tenant "acme", teams ["alpha"], user "u-sarah"}, a query_vector of dimension MEMORY_EMBED_DIM, and keyword_terms ["orders"]
    Then a Candidate for that fact is returned
    And its semantic_score and keyword_score are both in [0.0, 1.0]

  Scenario: Edge case — cross-tenant isolation on recall
    Given a fact F owned by tenant "acme" with visibility "tenant-shared"
    And a caller ctx whose tenant is "globex"
    When recall is called with ctx for tenant "globex" matching F's content
    Then F is not in the returned candidates
    And no Candidate references any record outside namespace "globex"

  Scenario: Edge case — supersession ends validity without deleting
    Given a currently-valid fact "fact:old" in tenant "acme" with valid_to NONE
    And a replacement fact "fact:new" in tenant "acme"
    When supersede is called with old "fact:old", new "fact:new", at "2026-01-10T12:00:00.000Z"
    Then get_fact for "fact:old" still returns the record
    And "fact:old".valid_to equals "2026-01-10T12:00:00.000Z" and "fact:old".superseded_by equals "fact:new"
    And "fact:new".supersedes equals "fact:old"

  Scenario: Edge case — verifiable hard delete removes derived summaries and embeddings
    Given a fact "fact:base" in tenant "acme" with two consolidated insights whose derived_from includes "fact:base"
    When hard_delete is called for "fact:base"
    Then a DeletionProof is returned with derived_removed listing both insight ids
    And embeddings_removed equals the count of removed embedding vectors
    And digest equals the sha256 hex of the sorted removed record ids
    And get_fact for "fact:base" returns None

  Scenario: Edge case — per-tenant erasure removes only the named tenant's rows
    Given a namespace holding facts, entities, audit entries and queue jobs for tenant "acme"
    And the same namespace holding the same kinds of row for a co-resident tenant "globex"
    When erase_tenant is called for "acme"
    Then a TenantErasureProof is returned whose per-table counts sum to total_removed
    And no row carrying owner.tenant, scope.tenant, grantor.tenant or tenant equal to "acme" remains in any table
    And every row belonging to "globex" is still present and unchanged
    And digest equals the sha256 hex of the tenant, the namespace and the sorted table counts

  Scenario: Error path — the namespace drop is refused where the namespace is not exclusive
    Given a deployment profile under which several tenants share one namespace
    When drop_tenant_namespace is called for "acme"
    Then the call returns StoreError::NamespaceNotExclusive
    And the mapped response is 403 with code SCOPE_FORBIDDEN
    And the namespace still exists with every tenant's rows intact

  Scenario: Error path — store unavailable
    Given the embedded store connection has been lost
    When get_fact is called
    Then the call returns StoreError::Unavailable
    And the mapped response is 503 with code STORE_UNAVAILABLE

  Scenario: Error path — score out of range on write
    Given a fact whose confidence is 1.4
    When put_fact is called
    Then the call returns StoreError::Validation
    And the mapped response is 400 with code VAL_OUT_OF_RANGE
    And no record is written
```

#### Performance, Security, Observability

- **Performance targets:** ANN-only vector similarity search ≤ 50 ms at target corpus size (NFR-P3, the
  vector portion of step 9). The full `recall` stage-1 operation and every other read-path store op fit
  the SA-LAT-01 stage-1 sub-budget (stage-1 ANN ≤ 50 ms within NFR-P2 p95 ≤ 200 ms). Per-statement
  timeout default 2000 ms; embedded in-process access removes any client/server hop (ADR-009). Memory:
  the HNSW index is held by the embedded engine; no per-request fact cache is kept in this component.
  `erase_tenant` is off the read path and carries no latency target: no index exists on the tenant
  field of any table, so each of the twelve tables is scanned twice (count, then re-count after the
  delete). At one tenant per namespace the scans see only that tenant's rows; under co-residency they
  see the whole namespace. Indexing the tenant field is not proposed here — it would add twelve
  indexes to every namespace to speed an operation each tenant invokes at most once, and the
  per-namespace footprint is the cost the consumer profile is trying to reduce.
- **Security:** every read/mutate method takes a `ScopeContext` and applies the read-filter rule,
  whose every arm carries `owner.tenant = $ctenant`. Cross-tenant access is structurally impossible
  wherever one namespace holds one tenant (ADR-011), and is prevented by that conjunct where they are
  co-resident (ADR-018) — the filter does not rely on the topology to be correct. The same holds on
  the destructive path: `erase_tenant` is bounded by the tenant predicate, and the unbounded namespace
  drop refuses to run outside a single-tenant namespace. Scope values come
  only from `ScopeContext` (derived by C3 from the validated token), never from request-body input. All
  queries are parameterised — caller content (`content`, `query_vector`, `keyword_terms`, names) is
  bound as parameters, never string-interpolated (no SQL/SurrealQL injection). The `audit_log` table is
  append-only by table permission and never stores the token or fact content (SA-AUDIT-01). No secrets,
  tokens, or raw PII are logged.
- **Observability:** the store emits no metrics. Log fields: `correlation_id`, `op`, `table`, `tenant_present`
  (boolean — never the tenant id at debug-and-below to avoid identifying data in operational logs),
  `outcome`. Trace spans: `store.connect`, `store.ensure_ns`, `store.put`, `store.get`, `store.recall`
  (child spans `store.recall.vector`, `store.recall.keyword`, `store.recall.graph`),
  `store.end_validity`, `store.supersede`, `store.hard_delete`, `store.audit`, `store.scan.decay`,
  `store.scan.contradiction`, `store.scan.reembed`, `store.scan.recent_episodes`,
  `store.update_maint`, `store.set_embedding`, `store.merge_entities`, `store.list_tenants`,
  `store.erase_tenant`, `store.drop_ns`, `store.drop_ns.refused`. `store.erase_tenant` carries
  `total_removed` and `tenant_present`, never the tenant id; `store.drop_ns.refused` records a namespace
  drop the exclusivity guard rejected and is logged at warn, because it means a caller reached for the
  unbounded path where only the bounded one is correct.

#### Change note — consumer tenancy profile, pass 1 (C11 dependency)

The Tenancy Benchmark Harness (C11, `docs/spec/components/tenancy-benchmark.md`) requires one change to
this component and no other: **`Migrator::render_up` widens from private to `pub(crate)`**
(`src/store/surreal/migrate.rs`), so the harness's database-per-tenant leg reuses the same statement
renderer — including the `<dim>` substitution from `MEMORY_EMBED_DIM` — rather than re-reading
`migrations/0001_init.up.surql` and drifting from it (SA-BENCH-RENDER-01). `pub(crate)` is the
narrowest visibility that reaches `src/bench`; C1's public surface is unchanged (rule C12).

C11 otherwise consumes only the already-public surface: `ensure_tenant_namespace`,
`drop_tenant_namespace`, `list_tenants`, `scan_recent_episodes`, `Migrator::new`,
`Migrator::migrate_up`, `Migrator::current_version`, `DB_NAME`, and `validate_tenant`. It adds no
table, no field, no index, and no migration, and it introduces no new call on any request path.

#### Change note — the benchmark measurement seams

Two additions, both for the re-pointed retrieval benchmark (`components/tenancy-benchmark.md`),
and neither changing any existing behaviour.

**`Store::open(endpoint, embed_dim, profile)` — `pub`.** Opens a store at an arbitrary endpoint under
an explicit profile, taking no `Config`. `new_in_memory_with_profile` now delegates to it.
`connect` remains the production path and stays configuration-derived; this one exists for a caller
that must inherit nothing, because the benchmark's first safety gate is that it never picks up a live
deployment's store configuration (SA-BENCH-ISOLATION-01), and because `new_in_memory_with_profile`
cannot reach `surrealkv://` — an absolute latency claim measured on `mem://` is not a claim about the
deployed engine. It performs no signin: it takes no credentials, so a secured remote endpoint belongs
on `connect`.

**`Store::recall_vector_only(ctx, q, strategy)` — `pub(crate)`.** The stage-1 vector signal alone,
under an explicitly named strategy. Two properties the measurement needs: one signal, so an NFR-P3
reading is not inflated by the keyword and graph legs or by the per-candidate `get_fact` resolution
`memory` performs afterwards; and the strategy as a *parameter* rather than read from the profile, so
both strategies can be timed against one store holding one corpus — comparing across two stores would
measure the data, not the strategy. `pub(crate)` is the narrowest visibility that reaches `src/bench`
(rule C12).

The vector leg is now a **single definition**: `recall` and `recall_vector_only` both call the private
`Store::vector_leg`, and the shared metadata fragment is `meta_clause`. A benchmark whose query has
drifted from the one production runs reports numbers that are worse than missing, because they get
quoted — so there is no second copy of the query to drift from, and
`vector_only_seam_agrees_with_the_public_recall_path` asserts the equivalence through the public path
regardless.

#### Change note — RISK-021 / ADR-018, per-tenant erasure

Per-tenant erasure was `drop_tenant_namespace`, and `REMOVE NAMESPACE` is equivalent to erasing one
tenant only while one namespace holds one tenant. The shared-namespace topology stage (ADR-018) maps many tenants onto one
namespace, at which point the same call destroys every co-resident tenant — irreversibly, on the path
a data subject invokes to exercise a right (RISK-021). Three rows of the HLD's regulatory table rested
on that equivalence, so the fix is an HLD decision (ADR-018) and not a code detail.

Two changes land here. `erase_tenant` is added as the erasure contract: a predicate-scoped delete over
the tables that carry a tenant (step 20a), verified by a re-count and reported as a `TenantErasureProof`.
`drop_tenant_namespace` is kept as the single-tenant-namespace mechanism and now refuses to run
anywhere else. Both are inert under today's topology — placement is still identity, so the guard
always passes and the predicate always covers exactly one namespace's worth of rows — which is the
point: the guarantee moves off the topology invariant before the topology changes, exactly as the
read-filter fix (RISK-017) moved the read guarantee off it.

This is the erasure half of the pair. The read half is the `owner.tenant = $ctenant` conjunct on every
read-filter arm; neither is sufficient alone under co-residency.

#### Gaps

None. (Reconciliation notes — no open gaps: FU-008 SurrealDB 3.x DDL drift, RISK-008 / FU-014
HNSW-KNN recall, RISK-009 maintenance-scope read filter, and FU-018 single squashed `0001_init`
migration are all reconciled to the as-built code above. The `recall` graph signal is the direct
`entities`-membership query over the `fact_ent` index, not multi-hop `relationship`-edge traversal;
this is the as-built behaviour, documented in the Vector/Keyword/Graph signal description.)

---

#### Amendment January 2026 — Collections, rank fusion and typed sub-queries (ADR-020)

> Source: HLD revision 0.11.0, ADR-020; requested by ordin's ADR-110 (decision 90), whose embedding contract is ordin's change M-8 (`docs/spec/ordin-desktop/components/MemoryService.md`, amendment January 2026). Every rule below adds to the sections above; where a rule replaces an earlier step, it says so.

##### Purpose (addition)

A fact may belong to a **collection** — a caller-chosen label that partitions one scope's facts into sets searched side by side (for ordin, one collection per team folder a workspace reads). The store filters by collection before any signal runs, ranks every signal separately within each collection, and fuses those ranked lists by reciprocal rank, so a collection of 200 facts is not crowded out of the stage-1 candidates by one of 20,000 whose raw scores happen to be higher. The fusion also replaces the as-built stage-1 cap, which kept candidates in fact-id order.

##### Approach (addition)

Rank is comparable across collections and across signals; raw scores are not — cosine similarity, a BM25 score normalised by the maximum of its own result set, and a fixed graph score of 1.0 have three different distributions, and one large collection sets the BM25 maximum for everyone. Reciprocal rank fusion (`Σ w / (60 + rank)`) is computed per (signal list, collection) run, so a collection's best match scores as its best match whatever the collection's size. Typed sub-queries arrive from C6 as further lists of one signal each. Rejected: *a global top-K by raw score, then a collection filter* — the over-fetch is clamped to 1,024 (step 9) and starves a small collection; *max-normalised score fusion* — the normalisation constant is set by the largest collection.

**As-built defect closed (ordin ADR-110).** Step 10 says "Return up to `q.stage1_k` candidates" without saying which. The code before this amendment merged the three signals into a `BTreeMap<String, (f64, f64, f64)>` keyed by bare fact id and took the first `stage1_k` entries in key order (`scores.into_iter().take(q.stage1_k as usize)`). Fact ids are UUIDv7, so when the three signals together surface more than `stage1_k` facts, the **oldest** facts are kept and the most relevant newer ones are dropped before the reranker sees them. The ordering was not intended; this amendment replaces it with step 10′ below. Recorded in HLD `10-risks.md` (cap-ordering risk, closed by ADR-020).

Assumptions (each listed in Phase 1 1A):

- **SA-COLL-01** `collection` is a string matching `^[a-z0-9:_-]{0,64}$`; `""` is the **default collection**, which every fact written before this amendment and every write that names none belongs to. The store never interprets the label.
- **SA-COLL-02** Per-collection lists are computed for every collection present among the read-filtered, currently-valid (or `valid_at`) facts when `filters.collections` is absent, and for exactly the listed ones when present. Absent the filter, at most 32 collections may be in play: with 33 or more, `recall` returns `StoreError::TooManyCollections { in_play }` before any run, and the caller gets `VAL_TOO_MANY_COLLECTIONS`; no collection is dropped to fit. Performance targets are stated for at most 32 collections in play.
- **SA-COLL-03** Whenever more than one collection is in play, or `filters.collections` is set, the vector leg of each collection runs `VectorStrategy::FilterFirstExact` restricted to that collection. A single-collection recall with no collection filter keeps `VectorStrategy::for_profile` exactly as today.
- **SA-COLL-04** RRF constant `k = 60`; bonus `0.05` to a fact ranked 1 in any run, else `0.02` to a fact ranked 2 or 3 in any run (applied once per fact).
- **SA-COLL-05** The SQL storage of ordin's change M-2 (`memory::store::sql`, DDL owned by `docs/spec/ordin-desktop/components/MemoryService.md` *Data Model* §B and its amendment January 2026) applies steps 9′, 10′ and 20c identically, with `mem_fact.collection` as the column. Its vector runs and its collections step read the search cache (*Search cache*, below), and it runs no `VectorStrategy` other than the exact scan.

##### Shared Context (additions)

```rust
// src/types/domain.rs — Fact gains one field (2C.2):
    /// Caller-chosen collection label (SA-COLL-01). "" = the default collection.
    /// `#[serde(default)]` so rows written before migration 0007 decode.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,

// src/types/api.rs — RecallFilters gains one field (2C.4; the C1 re-statement above is field-identical):
    /// 1–32 distinct labels, each matching SA-COLL-01; "" names the default collection.
    /// Absent = every collection the caller can read.
    #[serde(default)] pub collections: Option<Vec<String>>,

// src/types/ports.rs — StageOneQuery and Candidate (C1-owned) gain:
pub struct StageOneQuery {
    pub query_vector: Vec<f32>,          // plain query's vector; empty when `searches` is non-empty
    pub keyword_terms: Vec<String>,      // plain query's terms; empty when `searches` is non-empty
    pub searches: Vec<StageOneSearch>,   // typed sub-queries (C6 amendment); empty for a plain query
    pub filters: RecallFilters,
    pub scope: ScopeContext,
    pub stage1_k: u16,
}
#[derive(Clone)]
pub struct StageOneSearch {
    pub kind: SubQueryKind,              // Lex | Vec (2C.4, owned by C6's request type)
    pub vector: Vec<f32>,                // Vec only; len == MEMORY_EMBED_DIM
    pub terms: Vec<String>,              // Lex only; keyword_terms(sub-query text)
    pub weight: f64,                     // 2.0 for the first sub-query, 1.0 for the others
}
pub struct Candidate {
    pub fact_id: String,
    pub fact: Fact,                      // carries `collection`
    pub semantic_score: f64,             // max cosine over this fact's vector runs; 0.0 if none
    pub keyword_score: f64,              // max over its keyword runs of BM25 / max BM25 of that run; 0.0 if none
    pub graph_score: f64,                // 1.0 when reached by the graph run, else 0.0
    pub fused_score: f64,                // raw RRF sum plus bonus (step 10′)
    pub fused_norm: f64,                 // fused_score / fused_max, clamped to [0,1]
}
```

`Entity`, `Relationship` and `Source` (2C.2) gain the same field, with the same rule and default:

```rust
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,
```

A new trait, implemented by this component's store (and by the SQL storage of ordin's change M-2, whose `StoragePort` supertrait includes it — `docs/spec/ordin-desktop/components/MemoryService.md`, amendment January 2026, M-8 e):

```rust
// src/types/ports.rs
#[async_trait]
pub trait CollectionErasure: Send + Sync {
    /// Predicate-scoped erasure of one collection of one team scope (ADR-020; caller: ordin's
    /// Memory Service when a workspace stops reading a team folder). Removes every fact, entity,
    /// relationship and source with owner.tenant = tenant, owner.team = team and
    /// collection = collection, except an entity or source a remaining fact still names and a
    /// relationship between two remaining entities; and the collection's queued jobs, dead letters
    /// and held records, and the team's summaries of its facts (step 20c). `collection` "" is refused.
    async fn erase_collection(&self, tenant: &str, team: &str, collection: &str)
        -> Result<TenantErasureProof, StoreError>;
}
```

##### Public Interface (changes)

`recall(ctx, q)` keeps its signature; its behaviour is steps 9′ and 10′. `put_fact`, `put_entity`, `put_relationship` and `put_source` validate `collection` (SA-COLL-01) and refuse a breach with `StoreError::Validation("collection")`. The write pipeline gives a new entity or source the collection of the write that created it; an existing entity or source found by resolution keeps its own. `CollectionErasure::erase_collection` is added (above). No other method changes.

##### Internal Logic (changes)

**9′. `recall` — the runs (replaces step 9's three sub-queries; the read-filter, validity and metadata predicates are unchanged).**

1. Determine the collections in play, `C`: `q.filters.collections` when set, in the order listed; otherwise one parameterised query `SELECT collection FROM fact WHERE (<read-filter>) AND <validity><meta> GROUP BY collection` (rows lacking the field read as `""`), in ascending byte order of the label on every storage, the order fusion's tie-break on labels uses. The SQL storage answers this from the search cache, whose collections are held in that order; a database collation never decides it. The order reaches a result only through the graph cap (step 9″.5). Log `store.recall_collections { count }`.
2. Determine the **lists**: a plain query (`q.searches` empty) gives the vector list (weight 1.0, when `q.query_vector` is non-empty), the keyword list (weight 1.0, when `q.keyword_terms` is non-empty) and the graph list (weight 1.0, when `q.filters.entity` is set). A typed query gives one list per `StageOneSearch` — a vector list for `Vec`, a keyword list for `Lex` — with that search's `weight`, plus the graph list when `q.filters.entity` is set.
3. For every (list, collection `c` ∈ `C`) pair, run the list's signal query exactly as step 9 describes it, with the conjunct `collection = $c` added to the read-filter, validity and metadata predicates (filter-first: the collection predicate is applied before scoring, never after a top-K). The vector run uses the strategy of SA-COLL-03. On the SQL storage the runs are concurrent: a vector run over at least 2,048 cached facts runs on a blocking thread, a smaller one inline, and each keyword run on its own pool connection. They are fused in (list, collection) order, so the result, and the error a failed recall reports (the first in that order), are those of running them in sequence. Each run returns at most `q.stage1_k` facts ordered by its own score descending, ties by fact id ascending; a fact's **rank** in a run is its 1-based position.
4. A run that fails returns `StoreError::Timeout` / `StoreError::Unavailable` for the whole recall, as step 9 does (fail fast).

**Search cache (SQL storage, `src/store/sql/cache.rs`).** The in-memory form of ordin's "per-scope embedding cache" (M-8 g, SA-READ-10).

- **Scope and content.** One entry per tenant. It holds every fact of the tenant, with or without an embedding, grouped by collection in byte order of the label: owner user, owner team, visibility, memory class, `valid_from`, `valid_to`, `ingested_at`, `entities` in stored order, and the decoded embedding. Timestamps are compared at the precision the dialect stores (milliseconds on SQLite, microseconds on PostgreSQL).
- **Filters.** A vector run, the collections step and the graph and link runs of step 9″ apply `search_read_filter`, the validity predicate and the metadata predicates to this snapshot; the tenant is the key. `filters.entity` is not cached: its fact ids are read from `mem_fact_entity` once per recall.
- **Exact scan.** A vector run scores every embedding the filter admits, with no pre-selection and no row limit. The cosine is `dot / (√‖q‖² · √‖v‖²)`, each sum taken in index order in `f64`, 0 when a norm is 0, clamped to `[0,1]`. A stored embedding of the wrong length fails a run whose filter admits that fact, with the `StoreError::Internal` of step 3.
- **Loading.** A tenant is read whole on its first recall, in id-ordered pages of 5,000. Afterwards the facts a write named since are re-read by id before the next recall, or the whole tenant once they exceed both 2,000 and a quarter of its facts. A recall with only keyword lists over listed collections and no graph list loads nothing. A recall overtaken on every load attempt reads the tenant once for itself alone.
- **Invalidation.** After its commit, every write that changes a filter column, an embedding or `entities` names its facts: `put_fact`, `set_fact_embedding`, `end_validity`, `supersede`, `keep_fact`, `hard_delete`, `erase_entity`, `merge_entities`. `erase_tenant`, `erase_scope` and `erase_collection` drop the tenant's entry, and a load that began before the drop installs nothing.
- **Bound.** None: the cache grows with the tenant's facts, and its size at the 100,000-fact soft limit is ordin's R-108.
- **Coherence.** The cache is coherent only while this `SqlStorage` is the one writer of its tables, the embedded deployment it exists for. A write by another process, another `SqlStorage` over the same database or a hand-run statement is not seen until the tenant is reloaded.

**10′. Fuse and cap (replaces step 10).**

1. For each fact `f` in any run, `fused_score(f) = Σ_{runs r containing f} w_r / (60 + rank_r(f)) + b(f)`, where `w_r` is the weight of the run's list and `b(f) = 0.05` if `f` is rank 1 in any run, else `0.02` if it is rank 2 or 3 in any run, else `0`. A fact belongs to one collection, so all of its runs are runs of that collection.
2. `fused_max = Σ_{lists ℓ of step 9′.2} w_ℓ / 61 + 0.05` (the score of a fact ranked first in every list). `fused_norm(f) = min(1, fused_score(f) / fused_max)`.
3. Per-signal scores: `semantic_score` = the highest cosine similarity of `f` in a vector run; `keyword_score` = the highest, over its keyword runs, of its BM25 score divided by the maximum BM25 score of that run; `graph_score` = 1.0 if `f` is in the graph run. Each is 0.0 when absent.
4. **Selection.** Let `C⁺` be the collections with at least one fact in a run and `q = max(1, ⌊stage1_k / (2·|C⁺|)⌋)`. First, from each collection of `C⁺` take its `min(q, n_c)` facts of highest `fused_score` (ties by fact id ascending); when `|C⁺| > stage1_k`, only the `stage1_k` collections whose best `fused_score` is highest (ties by collection label ascending) contribute one fact each. Then fill the remaining places up to `stage1_k` from the facts not yet chosen, by `fused_score` descending, ties by fact id ascending.
5. Resolve each chosen id to its `Fact` through `get_fact(ctx, id)` (re-applying the read filter, as before), build the `Candidate`s, and return them ordered by `fused_score` descending, ties by fact id ascending. Logged at `store.recall` with `candidate_count`, `collections`, `runs`.

With one collection and no more than `stage1_k` facts across the runs, the candidate set equals the as-built set and the per-signal scores are unchanged (the keyword normalisation is the same per-run maximum); only the order and the cap rule differ.

**20c. `erase_collection(tenant, team, collection)`.**

1. `collection == ""` or breaking SA-COLL-01 → `StoreError::Validation("collection out of range")`. `ensure_and_use(tenant)`.
2. **Writes on their way first.** Delete the `work_job` and `dead_letter` rows with `scope.tenant = $tenant`, `scope.team = $team` and `payload.collection = $c`, and the `quarantine` rows of the collection, whoever wrote them, and count them under `work_job`, `dead_letter` and `quarantine` in `removed`. On the SQL storage these are the transaction's first statements; on the SurrealDB store they commit apart, before it. A worker either committed its fact before this, and it is erased below, or finds its job gone and takes the write back (`WritePipeline::process`). **Summaries next.** Collect the team's facts derived from the collection's facts, at any depth, inside or outside the collection, whoever wrote them; a folder erasure is not owner-bound, unlike step 13. Delete them with their junction rows and the relationships they leave without an asserting fact (step 13′), counted under `derived` and `relationship`. Then, in one transaction, every statement constrained by `owner.tenant = $tenant AND owner.team = $team AND collection = $c`: delete the `fact` rows; then the `entity` rows that no remaining fact of the team names; then the `source` rows that no remaining fact of the team cites; then the `relationship` rows of the collection, and any relationship of the team whose `from_ent` or `to_ent` was just deleted. The guard keeps an entity or source that a fact of another collection still names, so erasing one collection never breaks another. The `relationship_fact` rows of every removed fact and relationship go with them. Count rows removed per table. On every entity of the tenant that survives, `drop_folder_aliases(tenant, team, Some(collection))` takes this folder off `alias_folders` and removes each alias no other folder stated (`alias_folders`, Shared Context); a writer's entity can sit in another of their teams, so the rule is not limited to the erased team's entities.
3. Verify by re-count: any `fact` row of `(tenant, team, collection)` remaining → `StoreError::PartialDelete { removed, expected }`. Then call `sweep_summaries(erasure_scope(tenant, team), erased ids)` (step 20r).
4. Return `TenantErasureProof { erased_at: now, tenant, namespace: <the namespace used>, removed, total_removed, digest }`. `removed` has the keys `dead_letter`, `derived`, `entity`, `fact`, `quarantine`, `relationship`, `source` and `work_job`. Digest input lines: `tenant=<tenant>`, `namespace=<namespace>`, `team=<team>`, `collection=<collection>`, then one `<key>=<count>` per key in ascending key order (SA-ERASE-01's form with two lines added). Logged at `store.erase_collection` with counts only.

##### Data Model (additive migration)

Migration `0007_fact_collection` (registered after `0006`; `LATEST = 7`):

```sql
-- 0007_fact_collection.up.surql
DEFINE FIELD IF NOT EXISTS collection ON fact TYPE string DEFAULT ''
    ASSERT string::len($value) <= 64 AND string::matches($value, /^[a-z0-9:_-]*$/);
UPDATE fact SET collection = '' WHERE collection IS NONE;
DEFINE INDEX IF NOT EXISTS fact_collection ON fact FIELDS owner.team, collection;
-- the same field, backfill and index on entity, relationship and source:
DEFINE FIELD IF NOT EXISTS collection ON entity TYPE string DEFAULT ''
    ASSERT string::len($value) <= 64 AND string::matches($value, /^[a-z0-9:_-]*$/);
UPDATE entity SET collection = '' WHERE collection IS NONE;
DEFINE INDEX IF NOT EXISTS entity_collection ON entity FIELDS owner.team, collection;
DEFINE FIELD IF NOT EXISTS collection ON relationship TYPE string DEFAULT ''
    ASSERT string::len($value) <= 64 AND string::matches($value, /^[a-z0-9:_-]*$/);
UPDATE relationship SET collection = '' WHERE collection IS NONE;
DEFINE INDEX IF NOT EXISTS relationship_collection ON relationship FIELDS owner.team, collection;
DEFINE FIELD IF NOT EXISTS collection ON source TYPE string DEFAULT ''
    ASSERT string::len($value) <= 64 AND string::matches($value, /^[a-z0-9:_-]*$/);
UPDATE source SET collection = '' WHERE collection IS NONE;
DEFINE INDEX IF NOT EXISTS source_collection ON source FIELDS owner.team, collection;

-- 0007_fact_collection.down.surql
REMOVE INDEX IF EXISTS fact_collection ON fact;
REMOVE FIELD IF EXISTS collection ON fact;
REMOVE INDEX IF EXISTS entity_collection ON entity;
REMOVE FIELD IF EXISTS collection ON entity;
REMOVE INDEX IF EXISTS relationship_collection ON relationship;
REMOVE FIELD IF EXISTS collection ON relationship;
REMOVE INDEX IF EXISTS source_collection ON source;
REMOVE FIELD IF EXISTS collection ON source;
```

The same migration declares `collection` on `quarantine`, C4's SCHEMAFULL table (`write-pipeline.md`).

Rollback (sql-safety): the up path adds a field with a default to four tables and backfills `''` into existing rows — no row is lost and no value narrows. The down path removes the field definition and its index; `fact` is SCHEMALESS, so the stored values stay on the rows but are no longer validated or indexed, and a build without step 9′ searches every collection together. Forward-only in practice; the down path exists for the test harness. The SQL storage equivalent (`collection TEXT NOT NULL DEFAULT ''` on `mem_fact`, `mem_entity`, `mem_relationship`, `mem_source` and `mem_quarantine`; indexes `mem_fact_collection`, `mem_entity_collection`, `mem_relationship_collection` and `mem_source_collection`, each on `(tenant, owner_team, collection)`) is owned by ordin's Memory Service spec (SA-COLL-05).

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| `put_fact` with a `collection` breaking SA-COLL-01 | 400 | VAL_INVALID_BODY | `{"error":{"code":"VAL_INVALID_BODY","message":"invalid request body","correlation_id":"<uuid>"}}` |
| `erase_collection` with `""` or a malformed label | 400 | VAL_OUT_OF_RANGE | `{"error":{"code":"VAL_OUT_OF_RANGE","message":"value out of range","correlation_id":"<uuid>"}}` |
| `erase_collection` re-count finds a fact of the collection remaining | 500 | INTERNAL | `{"error":{"code":"INTERNAL","message":"internal error","correlation_id":"<uuid>"}}` |

No new registry code.

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Memory Store — collections and rank fusion

  Background:
    Given a namespace with team scope "ws:W" and the facts below, all team-shared and currently valid

  Scenario: Happy path — a small collection is not crowded out (fairness)
    Given collection "tk:a" holds 20,000 facts and collection "tk:b" holds 200
    And fact F of "tk:b" is the nearest of "tk:b" to query vector Q, with cosine 0.41, below the 50th-nearest fact of "tk:a" (cosine 0.62)
    When recall runs with Q, stage1_k 50 and no collection filter
    Then F is among the returned candidates
    And at least 12 candidates are from "tk:b"

  Scenario: Edge — a collection filter is applied before the search (no starvation)
    Given the same corpus
    When recall runs with filters.collections ["tk:b"] and stage1_k 50
    Then exactly 50 candidates are returned and every one is from "tk:b"

  Scenario: Edge — an exact rare term keeps its place
    Given fact R of "tk:a" contains the term "zq-7731" and no other fact does
    When recall runs a plain query whose keyword terms include "zq-7731"
    Then R's keyword run rank is 1, its fused_score includes the 0.05 bonus, and R is within the first 3 candidates by fused_score

  Scenario: Edge — one collection, small result: the as-built set is kept
    Given only the default collection, and the three signals together surface 37 facts with stage1_k 50
    When recall runs
    Then the 37 candidates are the same facts the as-built merge returned, with the same semantic, keyword and graph scores

  Scenario: Edge — the cap keeps the best, not the oldest
    Given only the default collection, and the signals together surface 140 facts with stage1_k 50
    When recall runs
    Then the 50 candidates are the 50 with the highest fused_score, whatever their fact ids

  Scenario: Happy path — typed sub-queries, first counts double
    Given a StageOneQuery with searches [Lex "orders table" (weight 2.0), Vec <vector of "who owns order data"> (weight 1.0)]
    When recall runs
    Then fused_max is 3/61 + 0.05 and a fact ranked 1 in both runs has fused_norm 1.0

  Scenario: Happy path — erasing a collection
    Given "tk:b" holds 200 facts naming 40 entities, 12 of which are also named by facts of "tk:a"
    When erase_collection("acme", "ws:W", "tk:b") runs
    Then the proof's removed["fact"] is 200 and removed["entity"] is 28
    And no fact of "tk:b" remains and every fact of "tk:a" remains

  Scenario: Error — erasing the default collection is refused
    When erase_collection("acme", "ws:W", "") runs
    Then it fails with StoreError::Validation whose detail contains "range" and nothing is deleted

  Scenario: Error — a malformed collection on write
    When put_fact is called with collection "Team B"
    Then it fails with StoreError::Validation and no row is written
```

##### Performance, Security, Observability (additions)

- **Performance.** Stage-1 including every run stays within the ANN sub-budget of ≤ 50 ms p95 (NFR-P3) for 3 collections of 20,000 facts each and up to 4 typed sub-queries, measured by the C11 per-collection leg (`tenancy-benchmark.md`, amendment January 2026). Runs per recall = lists × collections; filter-first exact vector runs cost O(visible facts of the collection) — recorded in HLD `10-risks.md` (per-collection query cost).
- **Security.** The collection predicate narrows the read filter and never widens it: the read-filter rule still decides what a caller can read, and a collection label grants nothing. Labels are logged as counts only (`collections`), never the labels themselves, because a caller may encode a name in them.
- **Observability.** `store.recall` adds `collections` and `runs`; `store.erase_collection` logs per-table counts and the digest.

##### Gaps

None.

---

#### Amendment January 2026 — Entities, links and graph recall (ADR-021)

> Source: HLD revision 0.12.0, ADR-021; requested by ordin's ADR-111 (decision 91), whose embedding contract is ordin's change M-9 (`docs/spec/ordin-desktop/components/MemoryService.md`, amendment January 2026). Counterparts: `write-pipeline.md` (writes entities and links), `retrieval-engine.md` (graph lists, trust weight, fading), `service-layer.md` (the new operations), each amendment January 2026. Every rule below adds to the sections above and to the January 2026 collections amendment; where a rule replaces a step, it says so.

##### Purpose (addition)

The graph the store has always declared is now written and read. A relationship records which facts asserted it and which ended it, closes when no live fact asserts it any more, and is removed when erasure leaves it without one. Recall finds the entities a query names by exact normalised name or alias, with no entity filter from the caller, and adds facts about entities up to two live links away as a further ranked list. The store also lists a scope's entities with their facts and links, returns a fact's versions, keeps a fact from fading, merges and erases entities, and deletes a single relationship with proof.

##### Approach (addition)

Graph signals join stage-1 as further ranked lists of the reciprocal rank fusion the collections amendment introduced (step 10′), so a link-reached fact competes on rank with vector and keyword matches and never on a raw score of its own; the graph list keeps its weight 1.0 and the link list weighs less (0.5 at one hop, 0.25 at two). A relationship is followed only through a fact the reader can read, so the graph cannot reveal what the read filter hides. Entity matching is exact on normalised names, because the store makes no model call (ADR-015) and exact matching has a predictable cost. Rejected: *edge traversal in SurrealQL graph syntax* — the SQL storage (ordin M-2) has no equivalent, and ADR-101 requires one behaviour on every backend; *fuzzy entity matching at read time* — a Dice scan over every entity name per query exceeds the stage-1 sub-budget at 2,000 entities; *storing a link's facts as a JSON array on the relationship* — closing a link on the last fact's end would need a read-modify-write per supersession, where a join table answers with one predicate.

Assumptions (each listed in Phase 1 1A):

- **SA-GRAPH-01** Names are normalised by `memory::text::normalise_name` (NFC, lower-case, trim, collapse whitespace, strip leading/trailing ASCII punctuation); `entity.names_norm` holds the normalised canonical name and aliases.
- **SA-GRAPH-03** Link kinds are normalised the same way minus punctuation stripping, and merged with an existing kind of the team scope by exact match, else bigram Dice ≥ 0.92 against `relationship_kinds(ctx, 1000)`.
- **SA-GRAPH-04** `end_validity` and `supersede` close every live relationship whose asserting facts of the relationship's own owner are all ended; `hard_delete` deletes a relationship left without an asserting fact of its owner. A teammate's fact neither ends a link nor keeps one live, under every scope, maintenance's included: the rule follows the fact, not the caller.
- **SA-GRAPH-05** A relationship is followed in recall only when at least one fact with role `asserted` passes the caller's fact read filter and is valid at the recall's time; a person-made relationship (no asserting fact, `replaces` set) is followed when its owner passes `novis_filter_clause()` and, when `owner_only` is set, the caller is its owner. A relationship with no asserting fact and no `replaces`, as an imported one can be, is stored, listed and exported, and is not followed.
- **SA-GRAPH-10** Query phrases are the 1- to 4-word n-grams of the normalised query text, at most 64, longest first; at most 8 matched entities.
- **SA-GRAPH-12** Entities written before migration `0008_graph` receive `names_norm` from `backfill_entity_names` at start; recall skips the graph and link lists of query-matched seeds until `maintenance_state.names_backfilled_at` is set. A seed given by `q.filters.entity` is not matched by name, so its lists run regardless.

##### Shared Context (additions)

```rust
// src/types/domain.rs — Relationship gains two fields (2C.2):
    /// The relationship a person's correction replaced (Service::correct_link, SA-GRAPH-06).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    /// A person's correction of a link no teammate could read: it stays the owner's, whatever its
    /// team, so correcting a private link does not publish it. The row keeps its team, because team
    /// and folder erasure find rows by it, and this flag narrows the read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owner_only: bool,

// src/types/domain.rs — Entity gains two fields and one type (2C.2):
    /// Those of `aliases` that came from the owner's private facts: stored apart, read by the owner
    /// alone, and for every other reader gone from `aliases` (`Entity::as_seen_by`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_aliases: Vec<String>,
    /// The folders that stated each alias. Store-internal: never serialised to a caller.
    #[serde(default, skip_serializing)]
    pub alias_folders: Vec<AliasFolder>,

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AliasFolder {             // one folder that stated an alias: a team's collection and the alias, normalised
    pub alias: String,
    #[serde(default)] pub team: Option<String>,
    #[serde(default)] pub collection: String,
}

// src/types/ports.rs — StageOneQuery and Candidate (C1-owned) gain:
pub struct StageOneQuery { /* … fields of the collections amendment … */
    pub graph_hops: u8,                  // MEMORY_GRAPH_HOPS, 0..=2
    pub graph_fanout: u32,               // MEMORY_GRAPH_FANOUT
    pub graph_cap: u32,                  // MEMORY_GRAPH_CANDIDATE_CAP
    pub phrase_texts: Vec<String>,       // the plain query, or each sub-query's text; entity phrases are cut from these
}
pub struct Candidate { /* … */
    pub source_trust: f64,               // the fact's source trust_signal; 1.0 when source_id is None
    pub graph_hop: Option<u8>,           // 0 = names a matched (or filter) entity; 1 or 2 = reached through links
    pub via: Vec<String>,                // relationship ids that reached it (hop ≥ 1), ≤ 4
}
```

**`alias_folders`.** An alias with no row here was not stated by a write, and no folder erasure removes it. An entity row written before migration `0009_job_fact` has none. `merge_entities` carries a folder row of `merge` over to `keep` when `keep` already holds a row for that alias, or does not yet hold the alias; `merge`'s canonical name gets no row, so no folder erasure removes it. A private alias made public by a shared statement stays public if that shared folder is erased while the private one survives.

A new trait, implemented by this component's store and by the SQL storage of ordin's change M-9 (its `StoragePort` supertrait includes it):

```rust
// src/types/ports.rs
#[async_trait]
pub trait GraphStore: Send + Sync {
    async fn find_live_relationship(&self, ctx: &ScopeContext, from: &str, kind: &str, to: &str) -> Result<Option<Relationship>, StoreError>;
    async fn relationship_kinds(&self, ctx: &ScopeContext, limit: u32) -> Result<Vec<String>, StoreError>;
    async fn link_fact(&self, ctx: &ScopeContext, relationship_id: &str, fact_id: &str, role: LinkRole) -> Result<(), StoreError>; // upsert
    async fn link_is_shared(&self, ctx: &ScopeContext, id: &str) -> Result<bool, StoreError>;
    async fn links_of_fact(&self, ctx: &ScopeContext, fact_id: &str) -> Result<Vec<(Relationship, LinkRole)>, StoreError>;
    async fn match_entities(&self, ctx: &ScopeContext, phrases: &[String], collections: Option<&[String]>, limit: u32) -> Result<Vec<Entity>, StoreError>;
    async fn neighbours(&self, ctx: &ScopeContext, seeds: &[String], hops: u8, fanout: u32, valid_at: Option<DateTime<Utc>>) -> Result<Vec<Neighbour>, StoreError>;
    async fn list_entities(&self, ctx: &ScopeContext, search: Option<&str>, cursor: Option<&str>, limit: u32) -> Result<EntityPage, StoreError>;
    async fn entity_detail(&self, ctx: &ScopeContext, entity_id: &str, limit: u32) -> Result<Option<EntityDetail>, StoreError>;
    async fn fact_versions(&self, ctx: &ScopeContext, fact_id: &str, limit: u32) -> Result<Vec<Fact>, StoreError>;
    async fn keep_fact(&self, ctx: &ScopeContext, fact_id: &str, salience_floor: f64) -> Result<(), StoreError>;
    async fn count_entity(&self, ctx: &ScopeContext, entity_id: &str) -> Result<EntityCount, StoreError>;
    async fn erase_entity(&self, ctx: &ScopeContext, entity_id: &str) -> Result<EntityErasureProof, StoreError>;
    async fn delete_relationship(&self, ctx: &ScopeContext, relationship_id: &str) -> Result<DeletionProof, StoreError>;
    async fn backfill_entity_names(&self, tenant: &str, batch: u32) -> Result<u64, StoreError>; // idempotent; 0 = done
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkRole { Asserted, Ended }
pub struct Neighbour { pub entity_id: String, pub hop: u8, pub via: String /* relationship id */ }
pub struct EntitySummary { pub entity: Entity, pub fact_count: u32, pub link_count: u32 }
pub struct EntityPage { pub items: Vec<EntitySummary>, pub next_cursor: Option<String> }   // by canonical_name, then id; 100 a page
pub struct EntityDetail { pub entity: Entity, pub facts: Vec<Fact>, pub links: Vec<Relationship>, pub names: HashMap<String, String> }
pub struct EntityCount { pub facts: u32, pub links: u32 }
pub struct EntityErasureProof { pub erased_at: DateTime<Utc>, pub entity_id: String, pub facts_removed: u32,
                                pub links_removed: u32, pub derived_removed: u32, pub digest: String,
                                #[serde(default)] pub held_removed: u32,    // held writes removed with the entity
                                #[serde(default)] pub jobs_removed: u32,
                                #[serde(default)] pub entity_removed: bool, // false: another user's row still uses it (SA-GRAPH-13)
                                #[serde(default)] pub facts_remaining: u32,
                                #[serde(default)] pub links_remaining: u32 }   // queued and dead-lettered jobs removed with it
```

`DeletionProof` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub relationships_removed: Vec<String>` (ids of relationships `hard_delete` or `delete_relationship` removed), and, in the same form, `entities_removed` and `sources_removed` (ids `hard_delete` removed, step 13‴). `memory::text::normalise_name(s: &str) -> String` is exported from `src/text.rs` (moved from C4's rules tier, behaviour unchanged).

##### Public Interface (changes)

`GraphStore` is added (above). `end_validity`, `supersede`, `hard_delete`, `merge_entities`, `put_entity` and `recall` keep their signatures; their behaviour gains steps 11′, 12′, 13′, 19d′, 7′ and 9″ below. `put_relationship` validates `replaces` (a `relationship:<uuid>` id or absent) and the normalised `kind` (1..=128 chars). No other method changes.

##### Internal Logic (changes)

**7′. `put_entity` (adds to step 7).** Compute `names_norm` = the distinct `normalise_name` values of `canonical_name` and every public alias (empty results skipped), and `private_names_norm` = the normalised private aliases that are not also a public name, and write them with the row in the same `UPSERT`. The SQL storage replaces the entity's `mem_entity_name` rows in the same transaction, one row per name with `owner_only` true for a private one. `mem_entity_name.name_norm` may not be empty, so an entity all of whose names normalise to `""` gets one row holding the first 512 characters of its canonical name verbatim, which no normalised query phrase equals; `backfill_entity_names` then terminates on it.

**9″. `recall` — graph and link lists (adds to step 9′; replaces the graph list of 9′.2).**
1. With no `q.filters.entity` and `names_backfilled_at` unset for the tenant → no graph or link list; log `store.recall_graph_unavailable` once per process; continue with the other lists (SA-GRAPH-12).
2. **Seeds.** With `q.filters.entity` set, the seed is that entity. Otherwise phrases are cut from `q.phrase_texts` (SA-GRAPH-10) and `match_entities(ctx, phrases, q.filters.collections, 8)` gives the seeds; no seed → no graph or link list.
3. **Graph list** (weight 1.0): per collection of `C`, facts naming a seed (`$entity IN entities` for any seed, served by `fact_ent` with the `WITH INDEX fact_ent` hint as step 9 requires), read-filtered and valid at `valid_at` (or live), ordered by `ingested_at` descending, ties by fact id, at most `stage1_k`; each gets `graph_hop = 0`. The SQL storage runs this list and the link list over the search cache's snapshot, which holds each fact's `ingested_at` and `entities`, scanning each collection as a vector run does.
4. **Link list** (when `q.graph_hops ≥ 1`): `neighbours(ctx, seeds, q.graph_hops, q.graph_fanout, q.filters.valid_at)`; per collection, facts naming a reached entity that is not a seed, read-filtered and valid, ordered by hop then `ingested_at` descending, at most `stage1_k`. A hop-1 run weighs 0.5 and a hop-2 run 0.25 (two runs per collection when `graph_hops = 2`). Each fact gets `graph_hop` = the lowest hop it was reached at and `via` = the relationships that reached it (≤ 4).
5. The graph and link runs together contribute at most `q.graph_cap` distinct facts; above it, the lowest-ranked facts of the deepest hop's runs are dropped first, in `C`'s order (step 9′.1), then those of the nearer hop, then those of the graph runs.
6. Fusion is step 10′ with these runs as further lists; `fused_max` adds `w / 61` for each graph or link list that ran. A fact reached by both the graph list and the link list keeps both contributions.
7. Step 10′.5 resolves `source_trust` for every chosen candidate with one batched read of the cited sources (`SELECT id, trust_signal FROM source WHERE id IN $ids`), read by tenant only, because each cited source belongs to a fact the caller's read filter has already admitted; a missing source gives 1.0.

**`neighbours(ctx, seeds, hops, fanout, valid_at)`.** 1. `hops` outside 1..=2 → `StoreError::Validation(OutOfRange, "hops")`. 2. Hop h: select relationships with `from_ent` or `to_ent` in the frontier, owner passing `novis_filter_clause()`, valid at `valid_at` (or `valid_to IS NONE`), `owner_only` unset or `owner.user = ctx.user`, and (SA-GRAPH-05) either `replaces IS NOT NONE AND no relationship_fact row of role asserted` or an asserting fact passing `fact_read_filter(ctx)` and valid at the same time; order by `confidence` descending then id in byte order; take at most `fanout` distinct other endpoints not yet seen. 3. The next frontier is those endpoints. 4. Return each reached entity once with its lowest hop and one `via`. Logged at `store.neighbours` with `seeds`, `reached`, `hops`. A hub entity touches thousands of relationships and only the first `fanout` new endpoints count, so the SQL storage reads each end of each frontier entity through its ranked index, up to `fanout × 8` rows, and multiplies that limit by 4 until the hop fills `fanout` or no row was left out. The result equals that of considering every edge.

**`match_entities(ctx, phrases, collections, limit)`.** Select the entities whose `names_norm` contains a phrase, or whose `private_names_norm` does when the caller owns the entity (SQL: `mem_entity_name` rows, `owner_only` rows for the owner alone), under the Entity read filter of Shared Context, `[AND collection IN $collections]`. Every match is read (a query has at most 64 phrases); then order them by the length of the longest phrase each entity matched, descending, then id, and take `limit`. Logged at `store.match_entities` with counts only.

**`find_live_relationship`** — `SELECT * FROM relationship WITH INDEX relationship_live WHERE owner.team = $team AND from_ent = $from AND kind = $kind AND to_ent = $to AND valid_to IS NONE` (`$team` = `ctx.teams[0]`, or `NONE` for a user-private write), under the Relationship read filter of Shared Context, the caller's own row first, then the smallest id. A row the caller cannot read behaves as absent and the write gets a row of its own, so two owners may hold live rows of one `from –kind→ to`. **`link_is_shared`** — whether a teammate could read the relationship: a fact recording it, either role, is not private, or `replaces` is set and `owner_only` is not. `Service::correct_link` sets `owner_only` on the replacement of a link for which this is false. **`relationship_kinds`** — the distinct `kind` values of the caller's readable relationships, at most `limit`, by most recent `ingested_at`. **`link_fact`** — `UPSERT relationship_fact:[$rel, $fact, $role] CONTENT {…, at: now}`. **`links_of_fact`** — the relationships with a `relationship_fact` row for the fact, with that row's role, read-filtered.

**11′. `end_validity` (adds to step 11).** After the fact's `valid_to` is set, in the same transaction: for each relationship with a row `(rel, fact, asserted)`, `valid_to IS NONE` and the fact's owner as its owner, if no other fact of that owner with role `asserted` on it is currently valid, `UPDATE relationship SET valid_to = $at`. Logged `store.links_closed { count }`.

**12′. `supersede` (adds to step 12).** As 11′ for `old_id` at `at`, after the `new` fact's own links are already recorded (C4 writes links before it supersedes), so a link the new fact repeats stays live.

**13′. `hard_delete` (adds to steps 13–14).** The removal set's facts (the fact and each derived insight) also remove their `relationship_fact` rows. Of the relationships a removed fact of the relationship's own owner has a row for, every one with no row of role `asserted` from a remaining fact of that owner, and no `replaces`, is deleted with them, in the same `DELETE`; this is read while the removed facts still exist. Its id joins the sorted id list the digest is computed over and `DeletionProof.relationships_removed`. The re-read check also re-reads each removed relationship id.

**13‴. `hard_delete` — what only the removal set used (SA-DELETE-02, adds to steps 13–14).** The candidates are the entities and sources the removal set's facts name. A candidate goes when it is owned by the owner of a removed fact, and no fact outside the removal set (any owner, any validity), no relationship outside those removed by step 13′ (as an endpoint, or by `source_id` for a source) and no held record (`quarantine.entities`, `quarantine.source_id`) uses it. The check runs in the delete's own transaction, an entity's name rows go with it, and the ids join `entities_removed`, `sources_removed` and the digest. A candidate owned by someone else stays: the owner rule of SA-AUTHZ-07 holds. The store records no creator, so this reads "the fact created it" as "nothing else uses it". A write resolving to the entity while it is deleted stores a fact naming an id that has gone; the next write of that name makes a new entity. A source's `trust_signal` goes with it: citing the origin again starts at `MEMORY_SOURCE_TRUST_DEFAULT`, so a note replaced by delete-then-write can be held where the first was admitted.

**13″. `hard_delete` — the job that wrote a fact (adds to step 14).** A job's retry would write a deleted fact back, and a failed job's payload holds its raw content. `drop_jobs_of(tenant, facts)` reads the `job_fact` rows of the facts and removes each named job, the `dead_letter` row with that `original_job_id`, and the job's `job_fact` rows. The job goes whole, which frees its idempotency key, as the other erasures do. On the SQL storage this runs inside the delete's transaction; on the SurrealDB store the jobs go in their own commit just before it. A worker holding such a job finds it gone and takes its write back. `WorkQueue::link_job_fact` writes the row just before the fact is stored; `WorkQueue::complete` removes the job's rows; a worker's own take-back removes them first through `WorkQueue::unlink_job_facts`, so it keeps its job.

**19d′. `merge_entities` (adds to step 19d).** In the same transaction: recompute `keep_id.names_norm` and `private_names_norm` from the folded aliases; repoint relationships; then, for any two live relationships of one `owner.user` that became identical `(from_ent, kind, to_ent)`, keep the one with the earlier `valid_from` (ties: smaller id), move the other's `relationship_fact` rows onto it, and delete the other. Two owners' rows stay two, so a caller's facts never move onto a link it cannot read. A relationship that became a self-loop (`from_ent = to_ent`) is deleted. When this plan (`merge_plan`) would delete a relationship the caller does not own, the whole merge is refused with `StoreError::ScopeForbidden` and nothing changes (`refuse_merge_of_others`); a maintenance scope passes.

**20d. `list_entities(ctx, search, cursor, limit)`.** `limit` 1..=100. Read-filtered entities ordered by `canonical_name`, then id, keyset-paged by `(canonical_name, id)` encoded base64url; `search` (1..=200 chars) matches when its normalised form is a substring of any `names_norm` element. For each entity, `fact_count` = currently-valid readable facts naming it, `link_count` = live readable relationships touching it (two grouped counts over the page's ids, never one query per entity).

**20e. `entity_detail(ctx, id, limit)`.** `limit` 1..=200. The entity (read-filtered, else `Ok(None)`); its currently-valid readable facts, newest `ingested_at` first; its relationships live and ended, newest `valid_from` first; `names` maps every link endpoint id to its canonical name.

**20f. `fact_versions(ctx, id, limit)`.** `limit` 1..=50. From the fact, follow `supersedes` back and `superseded_by` forward, each hop read-filtered; stop at `limit` or a missing or unreadable link. Return oldest first.

**20g. `keep_fact(ctx, id, salience_floor)`.** Read access via `fact_read_filter` (else `NotFound`); `valid_to` set → `NotFound`; `owner_gate`; `UPDATE fact SET memory_class = 'semantic', salience = math::max([salience, $floor])`. Idempotent. Logged at `store.keep_fact`.

**20h. `count_entity(ctx, id)`.** Readable entity (else `NotFound`); `facts` = readable facts naming it, any validity; `links` = readable relationships touching it, any validity.

**20i. `erase_entity(ctx, id)`.** Readable entity (else `NotFound`); `owner_gate`. Collect the caller's own facts naming it (any validity, `fact_read_filter`, `owner_view`), the caller's own consolidated facts derived from those, the caller's own relationships touching it, and the caller's own writes on their way that name it (`writes_naming`): queued and dead-lettered jobs whose payload names its canonical name or an alias in `content.subject`, `content.object`, a stated entity or a link end, and held records that do the same or list the entity id. Another user's queued write naming the entity writes that user's own entity and is not the caller's to remove. The writes on their way go first, as in step 20c. In one transaction delete those facts, their `relationship_fact` rows, those relationships and any other relationship left with no asserting fact (13′). Count what remains (SA-GRAPH-13): facts of the tenant still naming the entity and relationships of the tenant still touching it, readable by the caller or not. Both zero: the entity goes, `entity_removed = true`. Otherwise the entity stays for those rows, `entity_removed = false`, and `facts_remaining` and `links_remaining` carry the counts; the proof carries no id, owner or content of them. This is an outcome, not a failure. A row of the removal set still present after the delete → `StoreError::PartialDelete`, and nothing stays removed on the SQL storage, whose check runs before its commit. Then call `sweep_summaries(ctx, removed facts)` (step 20r). Digest: SHA-256 hex over the newline-joined sorted removed ids: facts, relationships, the entity when it went, and the writes on their way. `held_removed` counts the held records and `jobs_removed` the queued and dead-lettered jobs. Logged at `store.erase_entity` with counts only.

**20j. `delete_relationship(ctx, id)`.** Readable relationship (else `NotFound`); `owner_gate`; delete it and its `relationship_fact` rows; re-read; build `DeletionProof { record_id: id, derived_removed: [], embeddings_removed: 0, relationships_removed: [id], digest }`.

**20k. `backfill_entity_names(tenant, batch)`.** Select up to `batch` entities with `names_norm IS NONE`, compute and write `names_norm`, return the count (SQL: entities with no `mem_entity_name` row). When a call finds none and returns 0, set `maintenance_state.names_backfilled_at = now` only while it is unset, so the first completed backfill is the one recorded; the C7 cycle calls this every time. Called by the embedding caller at start in a loop (ordin: the Memory Service's start step) and by the C7 cycle while unset.

**20l. Team import and export (ordin M-7, via C9).** `put_relationship` stays an upsert by id (M-7 already requires it of the SQL storage) so an imported relationship replaces a stored one of the same id, under the owner rule of step 7; C9's `import_facts` then calls `link_fact(.., Asserted)` for each imported fact naming both endpoints, and its `export_scope` reads a page's relationships through `links_of_fact` for each fact of the page (either role, any validity, read-filtered), deduplicated by id. A relationship imported with no asserting fact is stored, listed and exported, and is not followed in recall (SA-GRAPH-05). Steps 20m to 20q are the other store methods they use.

**20m. `list_facts_page(ctx, after, include_retired, limit)`.** Read-filtered facts, newest `ingested_at` first (then id, descending), strictly after the keyset `after = (ingested_at, id)` of the previous page's last fact, at most `limit`. Facts whose validity has ended are left out unless `include_retired`. Read by `Service::list_facts` (ordin M-6 b).

**20n. `export_facts_page(ctx, after_id, limit)`.** Read-filtered facts of every validity ordered by id, strictly after `after_id`, at most `limit`. Read by `export_scope` (ordin M-7 a).

**20o. `relationships_among(ctx, entity_ids)`.** Read-filtered relationships of every validity whose `from` and `to` are both in `entity_ids`.

**20p. `set_pii_review(ctx, id, pii_review)`.** Set `pii_review` on one readable fact (else `NotFound`), after `owner_gate`, leaving every other field and the embedding as they are; `put_fact` would replace the row and clear the embedding. Called by `clear_pii_review` (ordin M-7 c).

**20q. `record_owner(tenant, id)`.** The `owner` of the row `id` names in `fact`, `entity`, `relationship` or `source`, chosen by the id's `table:` prefix and read past the read filter, because no other operation tells "absent" from "unreadable", which an import naming its own id needs. `None` when no such row exists. The owner never reaches a caller; the import reports only that the id is refused.

**20r. `sweep_summaries(ctx, gone)`.** Every erasure calls this after it commits: `hard_delete`, `erase_entity`, `erase_collection` and `erase_scope`. A summary committing while the delete runs is missed by the delete's own read, and held and failed work is outside its transaction. 1. `gone` empty → `Ok(0)`. 2. `drop_jobs_of(tenant, gone)` (step 13″). 3. Delete the `quarantine` rows whose `derived_from`, and the `dead_letter` rows whose `payload.derived_from`, name a fact of `gone`: the caller's own under a user's scope; those of `ctx.teams` under a maintenance scope. A held summary of another owner stays under the first rule. 4. Read the stored facts derived from `gone` through `fact_read_filter(ctx)`, keep the caller's own (`owner_view`), under a maintenance scope those of `ctx.teams`, and `hard_delete` each; `NotFound` is ignored, and each such delete sweeps in turn. Return how many went. A failure here is returned as an error even though the erasure it follows has committed. A team or folder erasure calls this under `erasure_scope(tenant, team)`: a maintenance-shaped scope whose `teams` is the one team, so it is not bound to one owner and stays within the team.

**20s. `erase_scope(tenant, team)` (ordin M-6 c).** A predicate delete of every row of `tenant` whose owner team is `team`, in `audit_log`, `dead_letter`, `entity`, `fact`, `grant`, `job_fact`, `quarantine`, `relationship`, `source` and `work_job` (the erasure set of step 20a without `idempotency_record` and `maintenance_state`, which carry no team; SQL: the same tables with the `mem_` prefix, plus `mem_entity_name`). `relationship_fact` rows of those facts and relationships go first, uncounted. Verify by a re-count on the same predicate that must find nothing, else `StoreError::PartialDelete`; the SQL storage does this before its commit. Then `drop_folder_aliases(tenant, team, None)` takes every folder of the team off the tenant's surviving entities, so the names that team stated on another team's entity go, and `sweep_summaries(erasure_scope(tenant, team), erased facts)` runs. Return `TenantErasureProof` with `namespace = team`, and digest lines `tenant=<tenant>`, `namespace=<team>`, then one `<table>=<count>` per table in key order. Logged at `store.erase_scope` with `total_removed` and `tenant_present`.

##### Data Model (additive migration)

Migration `0008_graph` (registered after `0007_fact_collection`; `LATEST = 8`):

```sql
-- 0008_graph.up.surql
DEFINE FIELD IF NOT EXISTS replaces ON relationship TYPE option<string>;
DEFINE INDEX IF NOT EXISTS relationship_live ON relationship FIELDS owner.team, from_ent, kind, to_ent;
DEFINE TABLE IF NOT EXISTS relationship_fact SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS relationship ON relationship_fact TYPE string;
DEFINE FIELD IF NOT EXISTS fact         ON relationship_fact TYPE string;
DEFINE FIELD IF NOT EXISTS role         ON relationship_fact TYPE string ASSERT $value IN ['asserted','ended'];
DEFINE FIELD IF NOT EXISTS at           ON relationship_fact TYPE datetime;
DEFINE INDEX IF NOT EXISTS relationship_fact_key  ON relationship_fact FIELDS relationship, fact, role UNIQUE;
DEFINE INDEX IF NOT EXISTS relationship_fact_fact ON relationship_fact FIELDS fact;
DEFINE FIELD IF NOT EXISTS names_norm ON entity TYPE option<array<string>>;
DEFINE INDEX IF NOT EXISTS entity_names ON entity FIELDS names_norm.*;
DEFINE FIELD IF NOT EXISTS names_backfilled_at ON maintenance_state TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS private_aliases    ON entity TYPE option<array<string>>;
DEFINE FIELD IF NOT EXISTS private_names_norm ON entity TYPE option<array<string>>;
DEFINE INDEX IF NOT EXISTS entity_private_names ON entity FIELDS private_names_norm.*;
DEFINE FIELD IF NOT EXISTS owner_only ON relationship TYPE option<bool>;

-- 0008_graph.down.surql
REMOVE FIELD IF EXISTS owner_only ON relationship;
REMOVE INDEX IF EXISTS entity_private_names ON entity;
REMOVE FIELD IF EXISTS private_names_norm ON entity;
REMOVE FIELD IF EXISTS private_aliases ON entity;
REMOVE FIELD IF EXISTS names_backfilled_at ON maintenance_state;
REMOVE INDEX IF EXISTS entity_names ON entity;
REMOVE FIELD IF EXISTS names_norm ON entity;
REMOVE TABLE IF EXISTS relationship_fact;
REMOVE INDEX IF EXISTS relationship_live ON relationship;
REMOVE FIELD IF EXISTS replaces ON relationship;
```

Migration `0009_job_fact` (registered after `0008_graph`; `LATEST = 9`):

```sql
-- 0009_job_fact.up.surql
DEFINE TABLE IF NOT EXISTS job_fact SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS job          ON job_fact TYPE string;
DEFINE FIELD IF NOT EXISTS fact         ON job_fact TYPE string;
DEFINE FIELD IF NOT EXISTS scope        ON job_fact TYPE object;
DEFINE FIELD IF NOT EXISTS scope.tenant ON job_fact TYPE string;
DEFINE FIELD IF NOT EXISTS scope.team   ON job_fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS scope.user   ON job_fact TYPE string;
DEFINE INDEX IF NOT EXISTS job_fact_fact ON job_fact FIELDS fact;
DEFINE INDEX IF NOT EXISTS job_fact_job  ON job_fact FIELDS job;
DEFINE INDEX IF NOT EXISTS dead_letter_job ON dead_letter FIELDS original_job_id;
DEFINE FIELD IF NOT EXISTS alias_folders ON entity TYPE option<array<object>>;

-- 0009_job_fact.down.surql
REMOVE FIELD IF EXISTS alias_folders ON entity;
REMOVE INDEX IF EXISTS dead_letter_job ON dead_letter;
REMOVE TABLE IF EXISTS job_fact;
```

`job_fact` links a fact to the job that wrote it (step 13″) and carries the job's scope, so both erasure sets cover it. Rollback (sql-safety): the up path adds one table, one field and one index on existing tables, with no backfill. The down path is **destructive for `job_fact`**: without it a delete leaves the raw payload of a failed or retrying job. `alias_folders` values stay on SCHEMALESS entity rows, unread. Forward-only in practice; the down path exists for the test harness.

Rollback of `0008_graph` (sql-safety): the up path adds optional fields, two indexes and one table; no row is changed and nothing narrows. The down path is **destructive for `relationship_fact`**: removing the table drops the record of which facts asserted each link, after which links no longer close when their facts end (SA-GRAPH-04) and erasure no longer removes them; restoring needs the pre-migration backup or a re-write of the facts. `names_norm` values stay on SCHEMALESS entity rows but are unindexed. Forward-only in practice; the down path exists for the test harness. The SQL storage equivalent (`mem_relationship.replaces` and `owner_only`, `mem_rel_live`, `mem_rel_from_rank`, `mem_rel_to_rank`, `mem_relationship_fact`, `mem_entity.private_aliases`, `mem_entity_name` with `owner_only`, `mem_entity_name_lookup`, `mem_entity_name_norm`, `mem_maintenance_state.names_backfilled_at`, and for step `0009_job_fact` `mem_job_fact`, `mem_job_fact_fact`, `mem_dead_letter_job`, `mem_entity.alias_folders`) is owned by ordin's Memory Service spec (SA-COLL-05 applies to it unchanged).

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| `neighbours` with `hops` outside 1..=2, `list_entities` / `entity_detail` / `fact_versions` with `limit` out of range | 400 | VAL_OUT_OF_RANGE | `{"error":{"code":"VAL_OUT_OF_RANGE","message":"value out of range","correlation_id":"<uuid>"}}` |
| `put_relationship` with an empty or over-long normalised `kind`, or a malformed `replaces` | 400 | VAL_INVALID_BODY | `{"error":{"code":"VAL_INVALID_BODY","message":"invalid request body","correlation_id":"<uuid>"}}` |
| `keep_fact`, `count_entity`, `erase_entity`, `delete_relationship` on an absent or unreadable record | 404 | NOT_FOUND | `{"error":{"code":"NOT_FOUND","message":"not found","correlation_id":"<uuid>"}}` |
| `erase_entity` or `delete_relationship` re-count finds a removed record still present | 500 | INTERNAL | `{"error":{"code":"INTERNAL","message":"internal error","correlation_id":"<uuid>"}}` |

No new registry code.

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Memory Store — entities, links and graph recall

  Background:
    Given a namespace with team scope "ws:W", facts team-shared, and names backfilled

  Scenario: Happy path — one fact, two entities, one link
    Given fact F naming entities Borealis (B) and packaging (P)
    When put_relationship writes "supplies" from B to P and link_fact(rel, F, asserted) runs twice
    Then one relationship B–supplies–P is live and relationship_fact holds one row for (rel, F, asserted)

  Scenario: Edge — a replaced fact closes its link
    Given the live link B–supplies–P asserted only by F
    When supersede(F, G, t) runs and G asserts no such link
    Then the link's valid_to is t
    And a recall with valid_at after t does not reach facts through that link
    And fact_versions(G) returns [F, G] and links_of_fact(F) still returns the link with role asserted

  Scenario: Edge — a link another fact still asserts stays live
    Given the link asserted by F and by H
    When end_validity(F, t) runs
    Then the link stays live

  Scenario: Happy path — the query names an entity (criterion 3 at store level)
    Given 1,000 facts, none containing the word "supplies", and fact F naming P
    When recall runs with phrase_texts ["who supplies packaging?"] and no entity filter
    Then match_entities returns P and F is in the graph run at rank 1

  Scenario: Edge — the hop limit holds
    Given links A–B, B–C, C–D asserted by fAB, fBC, fCD, and facts fB, fC, fD each naming one entity
    When recall runs with phrase_texts ["A"] and graph_hops 1
    Then fB is a candidate with graph_hop 1 and neither fC nor fD is a candidate

  Scenario: Error — a link through another workspace's fact is not followed (criterion 6)
    Given link B–C owned by team "ws:W" asserted only by a fact of team "ws:V"
    When a "ws:W" caller recalls naming B
    Then neighbours returns no C and no fact of "ws:V" is a candidate

  Scenario: Happy path — erasing an entity (criterion 14 at store level)
    Given entity E named by 12 facts and touched by 4 relationships
    When count_entity(E) runs
    Then it returns facts 12 and links 4
    When erase_entity(E) runs
    Then the proof has facts_removed 12 and links_removed 4, and match_entities finds nothing for E's names

  Scenario: Happy path — merging two entities (criterion 15 at store level)
    Given entities "Borealis Ltd" (K) and "Borealis" (M), each with facts and one identical live link to P
    When merge_entities(K, M) runs
    Then M is gone, "borealis" is in K's names_norm, one live link K–P remains with both facts' rows

  Scenario: Edge — keep stops fading
    Given an episodic fact F
    When keep_fact(F, 0.3) runs
    Then F's memory_class is semantic and its salience is at least 0.3

  Scenario: Edge — rows written before 0008 (criterion 19 at store level)
    Given 500 entities without names_norm
    When backfill_entity_names runs until it returns 0
    Then every entity has names_norm and names_backfilled_at is set, and every fact keeps its entities

  Scenario: Edge — an imported relationship replaces the stored one
    Given relationship R stored with confidence 0.6
    When put_relationship writes R with confidence 0.9 and link_fact(R, F, asserted) runs
    Then one row R with confidence 0.9 exists and relationship_fact holds (R, F, asserted)

  Scenario: Error — hard delete removes an orphaned link
    Given link L asserted only by fact F
    When hard_delete(F) runs
    Then the proof's relationships_removed is [L] and the digest covers L
```

The same scenarios run against ordin's SQL storage on SQLite and PostgreSQL through the component's test suite (ADR-101 item 4; ordin ADR-111 acceptance criterion 17).

##### Performance, Security, Observability (additions)

- **Performance.** Graph and link runs together ≤ 40 ms p95 at 10,000 facts, 20,000 relationships and 2,000 entities with `graph_hops = 1`, inside the whole-path p95 ≤ 200 ms (NFR-P2), measured by the C11 graph leg (`tenancy-benchmark.md`, amendment January 2026). Runs per recall gain at most 1 + `graph_hops` per collection. `list_entities` page of 100 ≤ 100 ms p95; `erase_entity` of 50 facts ≤ 2 s p95.
- **Security.** Every graph query applies the read filter to relationships and, through SA-GRAPH-05, to their asserting facts; a link grants nothing. Entity names, aliases and kinds are never logged; only counts and ids.
- **Observability.** `store.recall` adds `graph_seeds`, `graph_reached`; new log events `store.neighbours`, `store.match_entities`, `store.links_closed`, `store.erase_entity`, `store.keep_fact`, `store.recall_graph_unavailable`.

##### Gaps

None.

---

#### Amendment January 2026 — The name backfill at 20,000 entities

*Amended January 2026 (Approved by the user 2026-01-29, user decision, 29 January 2026). ordin's upgrade acceptance run (C-94) asks that the one-time name index finish within 60 s for 20,000 things. No store copied from an installation written before migration `0008_graph` exists, so a synthetic store in that shape stands in. Every rule below adds to the ADR-021 amendment above (SA-GRAPH-12, step 20k).*

- **SA-GRAPH-16** A store in the pre-`0008_graph` shape — 20,000 entities, each named by one currently-valid fact of its own, no relationships, and no normalised names (SQL: no `mem_entity_name` row; `names_backfilled_at` unset) — is upgraded by calling `backfill_entity_names(tenant, 500)` until it returns 0, as ordin's Memory Service does at start. Pass when the loop ends without an error within 60 s and its counts sum to 20,000; afterwards every fact reads back through `get_fact` naming its one entity, `count_entity` of every entity is 1 fact and 0 links, and `match_entities` finds an entity by its name. Measured on SQLite and on PostgreSQL by an `#[ignore]`d case of the storage conformance suite, run explicitly; only the backfill loop is timed, not the building of the store. The budget is set for the business laptop; a run on another host is recorded with that host's name and load.

```gherkin
  Scenario: Edge — 20,000 entities written before 0008 are indexed within a minute (SA-GRAPH-16)
    Given 20,000 entities each named by one fact, with no normalised names and no links
    When backfill_entity_names(tenant, 500) runs until it returns 0
    Then the loop takes at most 60 s and its counts sum to 20,000
    And every fact reads back naming its entity, with 1 fact and 0 links per entity
```

##### Gaps

None.

---

#### Amendment January 2026 — Triples, values, conflicts and passages (ADR-022)

> **Status:** Approved by the user January 2026. Source: HLD revision 0.13.0, ADR-022; requested by ordin's ADR-118 (decision 100), whose SQL DDL is ordin's change M-10 (`docs/spec/ordin-desktop/components/MemoryService.md`, amendment January 2026 — facts as triples). Counterparts: `write-pipeline.md` (states the triple, the change, the end and the conflicts), `service-layer.md` (validation, `fact_conflicts`), `retrieval-engine.md` (`RankedFact.conflicts`), `maintenance-worker.md` (`record_conflicts`), each amendment January 2026 (ADR-022). Every rule below adds to the sections above and to the ADR-021 amendment; where a rule replaces a step, it says so.

##### Purpose (addition)

A fact's content is now one subject–predicate–object triple whose object is an entity or a typed value. The store keeps the triple's parts in columns of their own beside the content, so that a write finds the live facts with the same subject and predicate by one indexed lookup; it records disagreeing facts as **conflicts** instead of ending one of them; and an erasure that removes the last fact naming an entity removes the entity and the person-made links that touched it. A fact the caller classes as a passage is stored as before.

##### Approach (addition)

The triple stays in `content` (the JSON the caller sent, returned unchanged), and the store derives indexed columns from it at `put_fact`, as it derives `names_norm` from an entity's names (step 7′). The subject and an entity object are stored by entity id, because two names of one entity must compare equal. A conflict is a row of its own (`fact_conflict`), not a field on either fact, so that it goes when either fact ends without a read-modify-write of the other. Rejected: *a field `conflicts_with` on the fact* — ending one fact would rewrite the other; *comparing `content` JSON at read time* — a scan per write over every live fact of the subject; *a `TYPE object` field for the object* — the SQL storage needs plain columns for its partial index, and both backends must behave alike (ordin ADR-101).

Assumptions (each listed in Phase 1 1A): **SA-TRIPLE-01** (the shape), **SA-TRIPLE-02** (value kinds and canonical form), **SA-TRIPLE-03** (a triple's text), **SA-TRIPLE-06** to **SA-TRIPLE-08** (change, end, conflict), **SA-TRIPLE-10** (an entity goes with its last fact), **SA-TRIPLE-11** (passages), **SA-TRIPLE-12** (earlier and imported facts).

##### Shared Context (additions)

```rust
// src/types/api.rs (2C.4) — the content of a non-passage write
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Triple { pub subject: String /* 1..=512 */, pub predicate: String /* 1..=128 */, pub object: TripleObject }
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TripleObject {
    Entity { name: String },                                       // 1..=512
    Text { value: String },                                        // 1..=1000
    Number { value: String, #[serde(default)] unit: Option<String> },   // decimal string; unit 1..=32
    Money { amount: String, currency: String },                    // decimal string; ^[A-Z]{3}$
    Date { value: String },                                        // YYYY | YYYY-MM | YYYY-MM-DD
    Boolean { value: bool },
}

// src/types/domain.rs — Fact gains (2C.2):
    /// The writer stated the predicate single-valued (ADR-022 (2)).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub single_valued: bool,
    /// The writer stated that the earlier value stopped being true (ADR-022 (2)).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub changed: bool,
    /// The triple's indexed parts, set by C4 after entity resolution. Store-internal: never serialised to a caller.
    #[serde(default, skip_serializing)] pub triple_key: Option<TripleKey>,

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TripleKey {
    pub subject_entity: String,          // "entity:<uuid>"
    pub predicate: String,               // memory::text::normalise_predicate
    pub object_kind: ObjectKind,
    pub object_value: String,            // canonical form, SA-TRIPLE-02
    pub object_unit: Option<String>,     // normalised unit, or the currency
    pub object_entity: Option<String>,   // set only for ObjectKind::Entity
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind { #[default] Entity, Text, Number, Money, Date, Boolean }

// src/types/ports.rs — a conflict as the store returns it
pub struct FactConflict { pub fact: String, pub with: String, pub recorded_at: DateTime<Utc> }
```

`memory::text::normalise_predicate(s)` = NFC, trim, collapse internal whitespace to one space; letter case is kept. `memory::text::predicate_words(p)` = `p` with `_`, `-` and every boundary between a lower-case and an upper-case letter replaced by one space (SA-TRIPLE-03).

The `MemoryStore` trait gains (implemented by this component's store and by the SQL storage of ordin's change M-10):

```rust
    /// The live facts of `owner`, of every visibility and collection, with this subject entity and predicate, newest
    /// `valid_from` first, at most `limit` (1..=64). Served by the index `fact_triple` (ADR-022 (4)).
    async fn live_same_subject_predicate(&self, ctx: &ScopeContext, owner: &ScopeRef,
        subject_entity: &str, predicate: &str, limit: u32) -> Result<Vec<Fact>, StoreError>;
    /// Upsert one conflict pair (ordered by id); a pair where either fact is not live is not written; a pair of two owners is refused.
    async fn record_conflict(&self, ctx: &ScopeContext, a: &str, b: &str, at: DateTime<Utc>) -> Result<bool, StoreError>;
    /// The conflict rows naming any of `ids` (1..=100) whose other fact the caller can read and is live.
    async fn conflicts_of(&self, ctx: &ScopeContext, ids: &[String]) -> Result<Vec<FactConflict>, StoreError>;
```

##### Internal Logic (changes)

**7″. `put_fact` (adds to step 7).** When `f.triple_key` is `Some`, write `subject_entity`, `predicate`, `object_kind`, `object_value`, `object_unit` and `object_entity` with the row, and `single_valued`, `changed` and `author` (the write's `RememberRequest.author`, SA-TRIPLE-15); when `None` (a passage, an earlier fact, an import whose content is not a triple), the six fields are `NONE`. A row whose `predicate` is set and whose `subject_entity` is not, or whose `object_kind = 'entity'` and `object_entity` is not set → `StoreError::Validation(InvalidBody, "triple")`. `content_text` of a triple is SA-TRIPLE-03's text; of any other content, as before. The SQL storage writes the same columns of `mem_fact` (ordin M-10).

**20t. `live_same_subject_predicate(..)`.** `limit` outside 1..=64 → `StoreError::Validation(OutOfRange, "limit")`. `SELECT * FROM fact WITH INDEX fact_triple WHERE owner.tenant = $tenant AND owner.team = $team AND owner.user = $user AND subject_entity = $subject AND predicate = $predicate AND valid_to IS NONE ORDER BY valid_from DESC, id DESC LIMIT $limit`, bound to `owner` alone: no visibility or collection test and not `fact_read_filter(ctx)`, because the facts compared are the owner's own and a conflict is returned only to a reader who may read both facts (20v; ADR-022 (4)). The caller narrows to one visibility and collection where it ends facts (write pipeline step 8″). One indexed lookup; logged at `store.same_subject_predicate` with `found`.

**20u. `record_conflict(ctx, a, b, at)`.** `a == b` → `StoreError::Validation(InvalidBody, "conflict")`. Order the pair so that `fact_a < fact_b` in byte order. In one transaction: read both facts past nothing but the tenant; either absent or with `valid_to` set → `Ok(false)`, nothing written; the two facts' owners (tenant, team, user) differ → `StoreError::Validation(InvalidBody, "conflict owner")`, nothing written (ADR-022 (4): a conflict never crosses owners); else `UPSERT fact_conflict:[$fact_a, $fact_b] CONTENT { fact_a, fact_b, tenant, owner, recorded_at: $at }` and `Ok(true)`. The owner is the two facts' common owner; their visibilities and collections may differ. Logged at `store.conflict_recorded` with the two ids.

**20v. `conflicts_of(ctx, ids)`.** `ids` 1..=100 → else `StoreError::Validation(OutOfRange, "ids")`. Select the `fact_conflict` rows with `fact_a` or `fact_b` in `ids`; keep a row when its other fact passes `fact_read_filter(ctx)` and is live; return one `FactConflict { fact, with, recorded_at }` for each id of `ids` on the row, ordered by `fact`, then `with`.

**11″. `end_validity` (adds to step 11′).** In the same transaction, delete every `fact_conflict` row naming the ended fact. **12″. `supersede` (adds to step 12′).** The same for `old_id`. **13⁗. `hard_delete` (adds to steps 13′–13‴).** Every `fact_conflict` row naming a fact of the removal set is deleted in the delete's transaction; the rows are not counted in the proof (they hold no content). Then step 13‴'s candidates are re-judged (SA-TRIPLE-10): an entity owned by a removed fact's owner that no fact outside the removal set names (any owner, any validity), that no held record names, and that is touched outside the removal set only by relationships of that owner with no `relationship_fact` row of role `asserted` (a person's correction, `replaces` set; an imported link) goes, and those relationships go with it. Their ids join `entities_removed`, `relationships_removed` and the digest, and the re-read check re-reads them. A relationship of another owner touching the entity keeps it (SA-AUTHZ-07).

**20i′. `erase_entity` (adds to step 20i).** The `fact_conflict` rows of the deleted facts go in the same transaction. **20s′. `erase_scope`.** `fact_conflict` joins the erasure set (SQL: `mem_fact_conflict`) and its rows go first, uncounted, as `relationship_fact` rows do: as in 13⁗, they are not counted in the digest or the proof (they hold no content). Step 20s's re-count also covers them, so a row left behind is the existing `StoreError::PartialDelete`. **20d′. `list_entities` and 20e′. `entity_detail`.** Unchanged: `fact_count` counts currently-valid readable facts naming the entity, passages included; the caller filters by `fact_class` when it must (ordin M-10).

**18 (replaced). `scan_contradiction_candidates(ctx, limit)`.** Pairs of currently-valid facts that share at least one entity id and whose `predicate` field is `NONE` on both (written before `0011_triples`), capped at `limit`. Facts written after the migration are compared at the write (SA-TRIPLE-09).

**Recall (adds to step 9″).** After the final candidate set is chosen, one `conflicts_of(ctx, ids)` read over the returned facts fills `RankedFact.conflicts` (`retrieval-engine.md`, amendment ADR-022); this read is outside stage-1's budget and inside the whole path's p95.

##### Data Model (additive migration)

Migration `0011_triples` (registered after `0010_link_author`; `LATEST = 11`):

```sql
-- 0011_triples.up.surql
DEFINE FIELD IF NOT EXISTS subject_entity ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS predicate      ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS object_kind    ON fact TYPE option<string>
    ASSERT $value IS NONE OR $value IN ['entity','text','number','money','date','boolean'];
DEFINE FIELD IF NOT EXISTS object_value   ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS object_unit    ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS object_entity  ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS single_valued  ON fact TYPE option<bool>;
DEFINE FIELD IF NOT EXISTS changed        ON fact TYPE option<bool>;
DEFINE FIELD IF NOT EXISTS author         ON fact TYPE option<string>;
DEFINE INDEX IF NOT EXISTS fact_triple ON fact FIELDS owner.team, subject_entity, predicate, valid_to;
DEFINE TABLE IF NOT EXISTS fact_conflict SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS fact_a       ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS fact_b       ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS tenant       ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS owner        ON fact_conflict TYPE object;
DEFINE FIELD IF NOT EXISTS owner.tenant ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS owner.team   ON fact_conflict TYPE option<string>;
DEFINE FIELD IF NOT EXISTS owner.user   ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS recorded_at  ON fact_conflict TYPE datetime;
DEFINE INDEX IF NOT EXISTS fact_conflict_key ON fact_conflict FIELDS fact_a, fact_b UNIQUE;
DEFINE INDEX IF NOT EXISTS fact_conflict_b   ON fact_conflict FIELDS fact_b;

-- 0011_triples.down.surql
REMOVE INDEX IF EXISTS fact_conflict_b ON fact_conflict;
REMOVE INDEX IF EXISTS fact_conflict_key ON fact_conflict;
REMOVE TABLE IF EXISTS fact_conflict;
REMOVE INDEX IF EXISTS fact_triple ON fact;
REMOVE FIELD IF EXISTS author ON fact;
REMOVE FIELD IF EXISTS changed ON fact;
REMOVE FIELD IF EXISTS single_valued ON fact;
REMOVE FIELD IF EXISTS object_entity ON fact;
REMOVE FIELD IF EXISTS object_unit ON fact;
REMOVE FIELD IF EXISTS object_value ON fact;
REMOVE FIELD IF EXISTS object_kind ON fact;
REMOVE FIELD IF EXISTS predicate ON fact;
REMOVE FIELD IF EXISTS subject_entity ON fact;
```

`fact_conflict_key` serves the lookup by `fact_a` and the uniqueness of a pair; `fact_conflict_b` serves the lookup by `fact_b` ("an index per fact", ADR-022). Rollback (sql-safety): the up path adds optional fields, one index and one table; no row is changed and nothing narrows, and no backfill runs (SA-TRIPLE-12). The down path is **destructive for `fact_conflict`**: the record of which facts disagree is lost and both facts stay live with nothing telling the caller that they disagree; restoring needs the pre-migration backup. The triple fields stay on SCHEMALESS rows, unindexed; the content still holds the triple. Forward-only in practice; the down path exists for the test harness. The SQL storage equivalent (`mem_fact` columns, `mem_fact_triple`, `mem_fact_conflict`, `mem_fact_conflict_b`) is owned by ordin's Memory Service spec (M-10; SA-COLL-05 applies unchanged).

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| `put_fact` with `predicate` set and `subject_entity` unset, or an entity object without `object_entity` | 400 | VAL_INVALID_BODY | `{"error":{"code":"VAL_INVALID_BODY","message":"invalid request body","correlation_id":"<uuid>"}}` |
| `record_conflict` with two facts of different owners | 400 | VAL_INVALID_BODY | `{"error":{"code":"VAL_INVALID_BODY","message":"invalid request body","correlation_id":"<uuid>"}}` |
| `record_conflict` with the same fact twice | 400 | VAL_INVALID_BODY | `{"error":{"code":"VAL_INVALID_BODY","message":"invalid request body","correlation_id":"<uuid>"}}` |
| `live_same_subject_predicate` `limit` or `conflicts_of` `ids` out of range | 400 | VAL_OUT_OF_RANGE | `{"error":{"code":"VAL_OUT_OF_RANGE","message":"value out of range","correlation_id":"<uuid>"}}` |
| `hard_delete` re-count finds a removed entity or relationship of 13⁗ still present | 500 | INTERNAL | `{"error":{"code":"INTERNAL","message":"internal error","correlation_id":"<uuid>"}}` |
| Store unreachable during any of the three new methods | 503 | STORE_UNAVAILABLE | `{"error":{"code":"STORE_UNAVAILABLE","message":"store unavailable","correlation_id":"<uuid>"}}` |

No new registry code.

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Memory Store — triples, conflicts and the last fact

  Background:
    Given a namespace with team scope "ws:W", facts team-shared, and names backfilled

  Scenario: Happy path — a triple's parts are stored and found by one lookup
    Given entity K "Kestrel" and entity T "Tom Reid"
    When put_fact writes F with content {"subject":"Kestrel","predicate":"accountManager","object":{"kind":"entity","name":"Tom Reid"}} and triple_key (K, "accountManager", entity, T)
    Then live_same_subject_predicate(K, "accountManager") returns [F]
    And F's content_text is "Kestrel account Manager Tom Reid"

  Scenario: Happy path — a conflict is recorded and read
    Given live facts F1 and F2 of one owner with subject K, predicate "accountManager" and different objects
    When record_conflict(F2, F1) runs
    Then one fact_conflict row (min(F1,F2), max(F1,F2)) exists
    And conflicts_of([F1]) returns {fact F1, with F2}

  Scenario: Edge — the lookup reaches every visibility and collection of the owner (ADR-022 (4))
    Given live facts F1 (visibility tenant-shared, collection "") and F2 (visibility team-shared, collection "team:folder-1") of one owner, both with subject K and predicate "accountManager"
    Then live_same_subject_predicate(owner, K, "accountManager") returns both

  Scenario: Error — a conflict across owners is refused
    Given live facts F1 of owner ws:W1 and F2 of owner ws:W2 with subject K and predicate "accountManager"
    When record_conflict(F1, F2) runs
    Then it fails with VAL_INVALID_BODY and no fact_conflict row is written

  Scenario: Edge — a conflict goes when one side ends
    Given the conflict of F1 and F2
    When end_validity(F1, t) runs
    Then no fact_conflict row names F1 and conflicts_of([F2]) is empty

  Scenario: Edge — a conflict with an ended fact is not written
    Given F1 live and F3 ended
    When record_conflict(F1, F3) runs
    Then it returns false and no row is written

  Scenario: Happy path — the last fact takes its entity and a person's link with it (SA-TRIPLE-10)
    Given entity P named only by fact F, and a person-corrected relationship R from P to K with no asserting fact
    When hard_delete(F) runs
    Then P and R are gone, the proof's entities_removed holds P and relationships_removed holds R, and the digest covers both

  Scenario: Edge — an entity another fact names stays
    Given entity K named by facts F and G
    When hard_delete(F) runs
    Then K stays and the proof's entities_removed does not hold K

  Scenario: Edge — earlier facts keep their shape
    Given a fact written before 0011_triples with content {"statement":"Borealis supplies packaging","subject":"Borealis"}
    When the store is migrated to 0011_triples
    Then the fact reads back unchanged with no triple fields, and scan_contradiction_candidates may return it

  Scenario: Edge — a passage stores no triple
    Given put_fact writes a fact with fact_class "passage" and triple_key None
    Then its predicate field is NONE and live_same_subject_predicate never returns it
```

The same scenarios run against ordin's SQL storage on SQLite and PostgreSQL through the component's test suite (ADR-101 item 4).

##### Performance, Security, Observability (additions)

- **Performance.** `live_same_subject_predicate` is one indexed lookup, p95 ≤ 5 ms at 20,000 live facts in the scope; `record_conflict` p95 ≤ 5 ms; `conflicts_of` for 20 ids p95 ≤ 5 ms. The write path gains one lookup per single-valued write, measured by the C11 write leg (`tenancy-benchmark.md`, amendment ADR-022).
- **Security.** `conflicts_of` returns only pairs whose other fact the caller can read, so a conflict never reveals a fact the read filter hides. Triple fields hold the same text the content holds; they are never logged.
- **Observability.** New log events `store.same_subject_predicate { found }`, `store.conflict_recorded { fact_a, fact_b }`; `store.end_validity` and `store.hard_delete` gain `conflicts_removed` (count).

##### Gaps

None.
