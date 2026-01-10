### SPEC: Memory Store

**File:** `src/store` | **Package:** `memory::store` | **Phase:** 1 | **Dependencies:** none

> **Mode:** greenfield
> **derivedFromHld:** 0.12.0

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
contract from ADR-004).

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
Fact/Entity/Relationship iff `record.owner.tenant == ctx.tenant` **and** ( `record.owner.user ==
ctx.user` **or** (`record.visibility == TeamShared` and `record.owner.team ∈ ctx.teams`) **or**
`record.visibility == TenantShared` **or** the caller holds an **active consent grant** over that Fact — one issued by its owner, naming the caller as grantee, unrevoked, and inside its validity window (`docs/spec/components/consent-grants.md`, SA-GRANT-01). The grant arm applies to Facts only, and never to the vector-search path (SA-GRANT-08). ).

The `record.owner.tenant == ctx.tenant` conjunct is **enforced in the predicate**, not merely implied by
the namespace. It was originally omitted from the SQL on the grounds that a different tenant is a
different namespace, which made it redundant — true under ADR-011, but it left the rule resting on a
topology invariant rather than on the filter, and RFC-03 Stage 2 removes that invariant on purpose
(RISK-017). All three arms needed it, not only the `TenantShared` one: `owner.user` and `owner.team`
are unique within a tenant, not across tenants, so co-resident id collisions would match too.

Entities and Relationships have no `visibility` field; for them the
filter reduces to `record.owner.tenant == ctx.tenant` **and** ( `record.owner.user == ctx.user` **or**
`record.owner.team ∈ ctx.teams` ).

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
(`owner.tenant = $ctenant AND (owner.user = $cuser OR owner.team IN $cteams)`) is used for
`entity`/`relationship`/`source` reads.

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
// src/store/types.rs   Defined here; referenced by name in §2C.6 and by C4/C6/C7.

/// Typed error for every MemoryStore operation. Maps to AppError::Store in §2C.7.
#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("validation: {1}")]                Validation(ValidationKind, String),    // -> 400 VAL_* (code per ValidationKind)
    #[error("not found")]                       NotFound,              // -> 404 NOT_FOUND
    #[error("scope forbidden")]                ScopeForbidden,        // -> 403 SCOPE_FORBIDDEN
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
take the record's own `owner` scope; they validate `owner.tenant` against the active namespace.

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
    async fn ensure_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>; // idempotent provision
    /// Per-tenant erasure (ADR-018): predicate-scoped delete of every row carrying `tenant`, across
    /// every table in the namespace it occupies, verified by a post-delete re-count. Correct whether
    /// the tenant has the namespace to itself or shares it.
    async fn erase_tenant(&self, tenant: &str) -> Result<TenantErasureProof, StoreError>;
    /// Namespace drop — the single-tenant-namespace mechanism only (ADR-011). Refuses with
    /// `StoreError::NamespaceNotExclusive` when the namespace may hold other tenants (ADR-018).
    async fn drop_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>;
    async fn ready(&self) -> Result<(), StoreError>; // connection live + vector-index dim == MEMORY_EMBED_DIM
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
   re-checked here; C8's `/readyz` compares `index_embed_dim()` against `MEMORY_EMBED_DIM`.

**`ensure_tenant_namespace(tenant)`:**

4. Delegate to `Migrator::migrate_up(tenant)`. The Migrator validates the tenant id
   (`validate_tenant`), creates the namespace and `recall` database itself with `DEFINE NAMESPACE IF
   NOT EXISTS <tenant>; USE NS <tenant>; DEFINE DATABASE IF NOT EXISTS recall;` (DDL identifiers cannot
   be parameter-bound, so the validated tenant is interpolated), ensures the `schema_migrations`
   bookkeeping table, then applies every pending numbered migration's `DEFINE TABLE`/`DEFINE
   FIELD`/`DEFINE INDEX`/`DEFINE ANALYZER` statements (Data Model). All `DEFINE` statements use `IF NOT
   EXISTS`, so the call is idempotent. Engine errors map via `map_db_err`
   (`StoreError::Timeout`/`StoreError::Unavailable`).

**`put_fact(f)` / `put_entity` / `put_relationship` / `put_source`:**

5. Validate the record against the domain rules in Shared Context (`entities.len() >= 1`; scores in
   `[0,1]`; `stability >= 0.0`; `valid_to >= valid_from`; `content` is a JSON object; string-length
   bounds). On failure return `StoreError::Validation` and do not write.
6. `USE NS <f.owner.tenant>` to select the namespace. If the configured/active namespace would differ
   from `f.owner.tenant`, return `StoreError::ScopeForbidden` (a record may not be written outside its
   own tenant namespace).
7. Execute a parameterised `UPSERT $id CONTENT $rec`, binding the record id as `$id` (a record-id
   value built from the "table:key" id, split on the first `:`) and the native row object as `$rec` —
   never string-interpolated. The row is built by `convert::*_to_object`; for `put_fact` it also
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
   `fact_filter_clause()`); `get_entity`/`get_relationship`/`get_source` use `novis_filter_clause()`.
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
    `StoreError::NotFound`. Run a parameterised `UPDATE <table>:⟨id⟩ SET valid_to = $at WHERE valid_to
    IS NONE`. Setting `valid_to` retires the record without deleting it (ADR-002). If the record was
    already retired (`valid_to` already set), the update is a no-op and returns `Ok(())` (idempotent).
    Logged at `store.end_validity`.

**`supersede(ctx, old_id, new_id, at)`:**

12. `USE NS <ctx.tenant>`. Confirm read access to both ids; if either is excluded or absent return
    `StoreError::NotFound`. In one parameterised transaction: `UPDATE fact:⟨old⟩ SET valid_to = $at,
    superseded_by = $new` and `UPDATE fact:⟨new⟩ SET supersedes = $old`. Neither fact is deleted
    (ADR-002). On store failure return `StoreError::Unavailable`. Logged at `store.supersede`.

**`hard_delete(ctx, id)` (SA-DELETE-01):**

13. `ensure_and_use(ctx.tenant)`. Confirm `forget` access path via `get_fact(ctx, id)` (the operation
    is authorised by C8; the store enforces tenant + read-filter scoping); if excluded or absent return
    `StoreError::NotFound`. Collect every consolidated-insight fact whose `derived_from` contains this
    id with a parameterised `SELECT * FROM fact WHERE $base IN derived_from AND (<read-filter>)`, served
    by the `fact_derived` index (added in migration `0003`; without it this query scans the whole `fact`
    table on an erasure path, where the work should be proportional to the lineage rather than to the
    namespace). No `WITH INDEX` hint is needed here, unlike the graph leg — this query carries no
    bi-temporal predicate, so there is no `fact_valid` for the planner to prefer. Adding one later would
    flip the choice, which is why the plan is asserted rather than assumed. This
    derived-collection query uses the maintenance-aware `fact_read_filter(ctx)` so a maintenance scope
    reaches user-private and team-shared derived insights (RISK-009). Build the removal set — the fact
    itself plus each derived insight — and count embedding vectors removed (the fact's own embedding
    plus any derived-summary embeddings, each probed via a `SELECT embedding FROM $id` non-null check).
14. Delete all collected records in one parameterised `DELETE $ids` (the ids bound as a list of
    record-id values). Compute `digest` as the SHA-256 hex of the newline-joined, sorted removed ids
    and build `DeletionProof { deleted_at: now, record_id: id, derived_removed: [...],
    embeddings_removed: N, digest }`. After deleting, re-read each collected id via `get_fact`; if any
    is still present the removal is partial — return `StoreError::PartialDelete { removed, expected }`
    (`removed = expected - still_present`) and never report the delete complete (SA-DELETE-01). Logged
    at `store.hard_delete` with `derived_removed_count`, `embeddings_removed`.

**`find_entity_by_name(ctx, name)` (entity-resolution support for C4):**

15. `USE NS <ctx.tenant>`. Run a parameterised `SELECT * FROM entity WHERE (canonical_name = $name OR
    $name IN aliases) AND <read-filter>`. Return all matches (the caller resolves ties). Logged at
    `store.find_entity`.

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
    both via `get_entity` (else `StoreError::NotFound`); in one `BEGIN; … COMMIT;` transaction repoint
    every `fact.entities` membership and every `relationship.from_ent`/`to_ent` string reference from
    `merge_id` to `keep_id` (the relationship endpoints are stored as the `from_ent`/`to_ent` string
    fields, not `from`/`to` — see Data Model), fold `merge_id`'s aliases + canonical_name into
    `keep_id.aliases`, then `DELETE $merge`. Logged at `store.merge_entities`.
19e. `list_tenants()`: `INFO FOR ROOT` to enumerate defined namespaces (the `namespaces` object's
    keys); return the tenant ids. Admin-only — takes no `ctx`; callers (C2 reaper, C7 scheduler)
    iterate the result and build a per-tenant `ScopeContext`. Logged at `store.list_tenants`.

**`erase_tenant(tenant)` (ADR-018 per-tenant erasure):**

20. Validate `tenant` (`validate_tenant`), then `ensure_provisioned(tenant)` so every table named
    below exists — erasing a tenant that was never written is a zero-count proof, not an error.
    Resolve the physical namespace once through `placement::resolve`; the whole erasure runs there.
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

    `schema_migrations` is deliberately absent: it records the namespace's schema version, not any
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
    costs less than the failure it prevents.
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
`migrations/0001_init.{up,down}.surql` before first release (greenfield, no shipped DB); the
`Migrator` registers exactly one entry (`version: 1, slug: "init"`) and the migration-count test
asserts `LATEST = 1` (FU-018). This component owns the C1 portion of that one migration — the `fact`,
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
"table:key" strings throughout (see `src/store/convert.rs`), so link-shaped fields are `string` /
`array<string>` / `option<string>`, not native record links.

```surql
-- src/store/migrate.rs SCHEMA_MIGRATIONS_DDL — created outside the numbered set so version 0 is readable
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
| `put_*` record `owner.tenant` differs from the active namespace (cross-tenant write attempt) | 403 | SCOPE_FORBIDDEN | `{"error":{"code":"SCOPE_FORBIDDEN","message":"record outside caller tenant","correlation_id":"<uuid>"}}` |
| Store connection lost / engine not reachable | 503 | STORE_UNAVAILABLE | `{"error":{"code":"STORE_UNAVAILABLE","message":"memory store unavailable","correlation_id":"<uuid>"}}` |
| Per-statement timeout (read-path store op exceeds budget; ANN > 50 ms or query > 2000 ms) | 504 | STORE_TIMEOUT | `{"error":{"code":"STORE_TIMEOUT","message":"memory store timed out","correlation_id":"<uuid>"}}` |
| `hard_delete` verification finds a collected record still present (partial delete) | 500 | INTERNAL | `{"error":{"code":"INTERNAL","message":"deletion incomplete","correlation_id":"<uuid>"}}` |
| `erase_tenant` verification finds rows still carrying the tenant after the delete (partial erasure) | 500 | INTERNAL | `{"error":{"code":"INTERNAL","message":"deletion incomplete","correlation_id":"<uuid>"}}` |
| `drop_tenant_namespace` called where the namespace may hold tenants other than the named one | 403 | SCOPE_FORBIDDEN | `{"error":{"code":"SCOPE_FORBIDDEN","message":"record outside caller tenant","correlation_id":"<uuid>"}}` |

`StoreError` → `code` mapping is owned by the Phase 4 Error Handling spec; this table states the
intended mapping for every failure mode the Internal Logic references. `StoreError::Validation` maps to
`VAL_INVALID_BODY` for structural failures and `VAL_OUT_OF_RANGE` for score-range failures;
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
  field of any table, so each of the eleven tables is scanned twice (count, then re-count after the
  delete). At one tenant per namespace the scans see only that tenant's rows; under co-residency they
  see the whole namespace. Indexing the tenant field is not proposed here — it would add eleven
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
- **Observability:** metrics `store_op_duration_seconds{op,table,outcome}`,
  `store_recall_candidates{tenant_present}`, `store_ann_duration_seconds`,
  `store_hard_delete_removed_total{kind}`. Log fields: `correlation_id`, `op`, `table`, `tenant_present`
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

#### Change note — RFC 02-consumer-tenancy-profile, pass 1 (C11 dependency)

The Tenancy Benchmark Harness (C11, `docs/spec/components/tenancy-benchmark.md`) requires one change to
this component and no other: **`Migrator::render_up` widens from private to `pub(crate)`**
(`src/store/migrate.rs:167`), so the harness's database-per-tenant leg reuses the same statement
renderer — including the `<dim>` substitution from `MEMORY_EMBED_DIM` — rather than re-reading
`migrations/0001_init.up.surql` and drifting from it (SA-BENCH-RENDER-01). `pub(crate)` is the
narrowest visibility that reaches `src/bench`; C1's public surface is unchanged (rule C12).

C11 otherwise consumes only the already-public surface: `ensure_tenant_namespace`,
`drop_tenant_namespace`, `list_tenants`, `scan_recent_episodes`, `Migrator::new`,
`Migrator::migrate_up`, `Migrator::current_version`, `DB_NAME`, and `validate_tenant`. It adds no
table, no field, no index, and no migration, and it introduces no new call on any request path.

#### Change note — plan 06 Phase 1, the benchmark measurement seams

Two additions, both for the re-pointed retrieval benchmark (`docs/wip/plan/06-re-pointed-retrieval-benchmark.md`),
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
tenant only while one namespace holds one tenant. RFC 03's topology stage maps many tenants onto one
namespace, at which point the same call destroys every co-resident tenant — irreversibly, on the path
a data subject invokes to exercise a right (RISK-021). Three rows of the HLD's regulatory table rested
on that equivalence, so the fix is an HLD decision (ADR-018) and not a code detail.

Two changes land here. `erase_tenant` is added as the erasure contract: a predicate-scoped delete over
the eleven tables that carry a tenant, verified by a re-count and reported as a `TenantErasureProof`.
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

**As-built defect closed (ordin ADR-110).** Step 10 says "Return up to `q.stage1_k` candidates" without saying which. The code merges the three signals into a `BTreeMap<String, (f64, f64, f64)>` keyed by bare fact id and takes the first `stage1_k` entries in key order (`memory/src/store/mod.rs:716`, `scores.into_iter().take(q.stage1_k as usize)`). Fact ids are UUIDv7, so when the three signals together surface more than `stage1_k` facts, the **oldest** facts are kept and the most relevant newer ones are dropped before the reranker sees them. The ordering was not intended; this amendment replaces it with step 10′ below. Recorded in HLD `10-risks.md` (cap-ordering risk, closed by ADR-020).

Assumptions (each listed in Phase 1 1A):

- **SA-COLL-01** `collection` is a string matching `^[a-z0-9:_-]{0,64}$`; `""` is the **default collection**, which every fact written before this amendment and every write that names none belongs to. The store never interprets the label.
- **SA-COLL-02** Per-collection lists are computed for every collection present among the read-filtered, currently-valid (or `valid_at`) facts when `filters.collections` is absent, and for exactly the listed ones when present. Performance targets are stated for at most 32 collections in play.
- **SA-COLL-03** Whenever more than one collection is in play, or `filters.collections` is set, the vector leg of each collection runs `VectorStrategy::FilterFirstExact` restricted to that collection. A single-collection recall with no collection filter keeps `VectorStrategy::for_profile` exactly as today.
- **SA-COLL-04** RRF constant `k = 60`; bonus `0.05` to a fact ranked 1 in any run, else `0.02` to a fact ranked 2 or 3 in any run (applied once per fact).
- **SA-COLL-05** The SQL storage of ordin's change M-2 (`memory::store::sql`, DDL owned by `docs/spec/ordin-desktop/components/MemoryService.md` *Data Model* §B and its amendment January 2026) applies steps 9′, 10′ and 20c identically, with `mem_fact.collection` as the column.

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

// src/store/types.rs — StageOneQuery and Candidate (C1-owned) gain:
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
    /// relationship between two remaining entities. `collection` "" is refused.
    async fn erase_collection(&self, tenant: &str, team: &str, collection: &str)
        -> Result<TenantErasureProof, StoreError>;
}
```

##### Public Interface (changes)

`recall(ctx, q)` keeps its signature; its behaviour is steps 9′ and 10′. `put_fact`, `put_entity`, `put_relationship` and `put_source` validate `collection` (SA-COLL-01) and refuse a breach with `StoreError::Validation(ValidationKind::InvalidBody, "collection")`. The write pipeline gives a new entity or source the collection of the write that created it; an existing entity or source found by resolution keeps its own. `CollectionErasure::erase_collection` is added (above). No other method changes.

##### Internal Logic (changes)

**9′. `recall` — the runs (replaces step 9's three sub-queries; the read-filter, validity and metadata predicates are unchanged).**

1. Determine the collections in play, `C`: `q.filters.collections` when set; otherwise one parameterised query `SELECT collection FROM fact WHERE (<read-filter>) AND <validity><meta> GROUP BY collection` (rows lacking the field read as `""`). Log `store.recall_collections { count }`.
2. Determine the **lists**: a plain query (`q.searches` empty) gives the vector list (weight 1.0, when `q.query_vector` is non-empty), the keyword list (weight 1.0, when `q.keyword_terms` is non-empty) and the graph list (weight 1.0, when `q.filters.entity` is set). A typed query gives one list per `StageOneSearch` — a vector list for `Vec`, a keyword list for `Lex` — with that search's `weight`, plus the graph list when `q.filters.entity` is set.
3. For every (list, collection `c` ∈ `C`) pair, run the list's signal query exactly as step 9 describes it, with the conjunct `collection = $c` added to the read-filter, validity and metadata predicates (filter-first: the collection predicate is applied before scoring, never after a top-K). The vector run uses the strategy of SA-COLL-03. Each run returns at most `q.stage1_k` facts ordered by its own score descending, ties by fact id ascending; a fact's **rank** in a run is its 1-based position.
4. A run that fails returns `StoreError::Timeout` / `StoreError::Unavailable` for the whole recall, as step 9 does (fail fast).

**10′. Fuse and cap (replaces step 10).**

1. For each fact `f` in any run, `fused_score(f) = Σ_{runs r containing f} w_r / (60 + rank_r(f)) + b(f)`, where `w_r` is the weight of the run's list and `b(f) = 0.05` if `f` is rank 1 in any run, else `0.02` if it is rank 2 or 3 in any run, else `0`. A fact belongs to one collection, so all of its runs are runs of that collection.
2. `fused_max = Σ_{lists ℓ of step 9′.2} w_ℓ / 61 + 0.05` (the score of a fact ranked first in every list). `fused_norm(f) = min(1, fused_score(f) / fused_max)`.
3. Per-signal scores: `semantic_score` = the highest cosine similarity of `f` in a vector run; `keyword_score` = the highest, over its keyword runs, of its BM25 score divided by the maximum BM25 score of that run; `graph_score` = 1.0 if `f` is in the graph run. Each is 0.0 when absent.
4. **Selection.** Let `C⁺` be the collections with at least one fact in a run and `q = max(1, ⌊stage1_k / (2·|C⁺|)⌋)`. First, from each collection of `C⁺` take its `min(q, n_c)` facts of highest `fused_score` (ties by fact id ascending); when `|C⁺| > stage1_k`, only the `stage1_k` collections whose best `fused_score` is highest (ties by collection label ascending) contribute one fact each. Then fill the remaining places up to `stage1_k` from the facts not yet chosen, by `fused_score` descending, ties by fact id ascending.
5. Resolve each chosen id to its `Fact` through `get_fact(ctx, id)` (re-applying the read filter, as before), build the `Candidate`s, and return them ordered by `fused_score` descending, ties by fact id ascending. Logged at `store.recall` with `candidate_count`, `collections`, `runs`.

With one collection and no more than `stage1_k` facts across the runs, the candidate set equals the as-built set and the per-signal scores are unchanged (the keyword normalisation is the same per-run maximum); only the order and the cap rule differ.

**20c. `erase_collection(tenant, team, collection)`.**

1. `collection == ""` or breaking SA-COLL-01 → `StoreError::Validation(ValidationKind::OutOfRange, "collection")`. `ensure_and_use(tenant)`.
2. In one transaction, every statement constrained by `owner.tenant = $tenant AND owner.team = $team AND collection = $c`: delete the `fact` rows; then the `entity` rows that no remaining fact of the team names; then the `source` rows that no remaining fact of the team cites; then the `relationship` rows of the collection, and any relationship of the team whose `from_ent` or `to_ent` was just deleted. The guard keeps an entity or source that a fact of another collection still names, so erasing one collection never breaks another. Count rows removed per table.
3. Verify by re-count: any `fact` row of `(tenant, team, collection)` remaining → `StoreError::PartialDelete { removed, expected }`.
4. Return `TenantErasureProof { erased_at: now, tenant, namespace: <the namespace used>, removed, total_removed, digest }` with digest input lines `tenant=<tenant>`, `namespace=<namespace>`, `team=<team>`, `collection=<collection>`, then one `<table>=<count>` per table in ascending key order (SA-ERASE-01's form with two lines added). Logged at `store.erase_collection` with counts only.

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

Rollback (sql-safety): the up path adds a field with a default to four tables and backfills `''` into existing rows — no row is lost and no value narrows. The down path removes the field definition and its index; `fact` is SCHEMALESS, so the stored values stay on the rows but are no longer validated or indexed, and a build without step 9′ searches every collection together. Forward-only in practice; the down path exists for the test harness. The SQL storage equivalent (`collection TEXT NOT NULL DEFAULT ''` on `mem_fact`, `mem_entity`, `mem_relationship` and `mem_source`, each indexed `(tenant, owner_team, collection)`) is owned by ordin's Memory Service spec (SA-COLL-05).

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
    Then it fails with StoreError::Validation(OutOfRange) and nothing is deleted

  Scenario: Error — a malformed collection on write
    When put_fact is called with collection "Team B"
    Then it fails with StoreError::Validation(InvalidBody) and no row is written
```

##### Performance, Security, Observability (additions)

- **Performance.** Stage-1 including every run stays within the ANN sub-budget of ≤ 50 ms p95 (NFR-P3) for 3 collections of 20,000 facts each and up to 4 typed sub-queries, measured by the C11 per-collection leg (`tenancy-benchmark.md`, amendment January 2026). Runs per recall = lists × collections; filter-first exact vector runs cost O(visible facts of the collection) — recorded in HLD `10-risks.md` (per-collection query cost).
- **Security.** The collection predicate narrows the read filter and never widens it: the read-filter rule still decides what a caller can read, and a collection label grants nothing. Labels are logged as counts only (`collections`), never the labels themselves, because a caller may encode a name in them.
- **Observability.** `store.recall` adds `collections` and `runs`; `store.erase_collection` logs per-table counts and the digest; metric `recall_stage1_runs` (histogram, labelled `{tenant}`).

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
- **SA-GRAPH-04** `end_validity` and `supersede` close every live relationship whose asserting facts are all ended; `hard_delete` deletes a relationship left without an asserting fact.
- **SA-GRAPH-05** A relationship is followed in recall only when at least one fact with role `asserted` passes the caller's fact read filter and is valid at the recall's time; a person-made relationship (no asserting fact, `replaces` set) is followed when its owner passes `novis_filter_clause()`.
- **SA-GRAPH-10** Query phrases are the 1- to 4-word n-grams of the normalised query text, at most 64, longest first; at most 8 matched entities.
- **SA-GRAPH-12** Entities written before migration `0008_graph` receive `names_norm` from `backfill_entity_names` at start; recall skips the graph and link lists until `maintenance_state.names_backfilled_at` is set.

##### Shared Context (additions)

```rust
// src/types/domain.rs — Relationship gains one field (2C.2):
    /// The relationship a person's correction replaced (Service::correct_link, SA-GRAPH-06).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,

// src/store/types.rs — StageOneQuery and Candidate (C1-owned) gain:
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

A new trait, implemented by this component's store and by the SQL storage of ordin's change M-9 (its `StoragePort` supertrait includes it):

```rust
// src/types/ports.rs
#[async_trait]
pub trait GraphStore: Send + Sync {
    async fn find_live_relationship(&self, ctx: &ScopeContext, from: &str, kind: &str, to: &str) -> Result<Option<Relationship>, StoreError>;
    async fn relationship_kinds(&self, ctx: &ScopeContext, limit: u32) -> Result<Vec<String>, StoreError>;
    async fn link_fact(&self, ctx: &ScopeContext, relationship_id: &str, fact_id: &str, role: LinkRole) -> Result<(), StoreError>; // upsert
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
                                pub links_removed: u32, pub derived_removed: u32, pub digest: String }
```

`DeletionProof` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub relationships_removed: Vec<String>` (ids of relationships `hard_delete` or `delete_relationship` removed). `memory::text::normalise_name(s: &str) -> String` is exported from `src/text.rs` (moved from C4's rules tier, behaviour unchanged).

##### Public Interface (changes)

`GraphStore` is added (above). `end_validity`, `supersede`, `hard_delete`, `merge_entities`, `put_entity` and `recall` keep their signatures; their behaviour gains steps 11′, 12′, 13′, 19d′, 7′ and 9″ below. `put_relationship` validates `replaces` (a `relationship:<uuid>` id or absent) and the normalised `kind` (1..=128 chars). No other method changes.

##### Internal Logic (changes)

**7′. `put_entity` (adds to step 7).** Compute `names_norm` = the distinct `normalise_name` values of `canonical_name` and every alias (empty results skipped) and write them with the row in the same `UPSERT`.

**9″. `recall` — graph and link lists (adds to step 9′; replaces the graph list of 9′.2).**
1. `names_backfilled_at` unset for the tenant → no graph or link list; log `store.recall_graph_unavailable` once per process; continue with the other lists.
2. **Seeds.** With `q.filters.entity` set, the seed is that entity. Otherwise phrases are cut from `q.phrase_texts` (SA-GRAPH-10) and `match_entities(ctx, phrases, q.filters.collections, 8)` gives the seeds; no seed → no graph or link list.
3. **Graph list** (weight 1.0): per collection of `C`, facts naming a seed (`$entity IN entities` for any seed, served by `fact_ent` with the `WITH INDEX fact_ent` hint as step 9 requires), read-filtered and valid at `valid_at` (or live), ordered by `ingested_at` descending, ties by fact id, at most `stage1_k`; each gets `graph_hop = 0`.
4. **Link list** (when `q.graph_hops ≥ 1`): `neighbours(ctx, seeds, q.graph_hops, q.graph_fanout, q.filters.valid_at)`; per collection, facts naming a reached entity that is not a seed, read-filtered and valid, ordered by hop then `ingested_at` descending, at most `stage1_k`. A hop-1 run weighs 0.5 and a hop-2 run 0.25 (two runs per collection when `graph_hops = 2`). Each fact gets `graph_hop` = the lowest hop it was reached at and `via` = the relationships that reached it (≤ 4).
5. The graph and link runs together contribute at most `q.graph_cap` distinct facts; above it, the lowest-ranked link-run facts are dropped first.
6. Fusion is step 10′ with these runs as further lists; `fused_max` adds `w / 61` for each graph or link list that ran. A fact reached by both the graph list and the link list keeps both contributions.
7. Step 10′.5 resolves `source_trust` for every chosen candidate with one batched read of the cited sources (`SELECT id, trust_signal FROM source WHERE id IN $ids`), read-filtered with `novis_filter_clause()`; a missing source gives 1.0.

**`neighbours(ctx, seeds, hops, fanout, valid_at)`.** 1. `hops` outside 1..=2 → `StoreError::Validation(OutOfRange, "hops")`. 2. Hop h: select relationships with `from_ent` or `to_ent` in the frontier, owner passing `novis_filter_clause()`, valid at `valid_at` (or `valid_to IS NONE`), and (SA-GRAPH-05) either `replaces IS NOT NONE AND no relationship_fact row of role asserted` or an asserting fact passing `fact_read_filter(ctx)` and valid at the same time; order by `confidence` descending then id; take at most `fanout` distinct other endpoints not yet seen. 3. The next frontier is those endpoints. 4. Return each reached entity once with its lowest hop and one `via`. Logged at `store.neighbours` with `seeds`, `reached`, `hops`.

**`match_entities(ctx, phrases, collections, limit)`.** `SELECT * FROM entity WITH INDEX entity_names WHERE names_norm CONTAINSANY $phrases AND (<novis read filter>) [AND collection IN $collections] LIMIT $limit`; order the result by the length of the longest phrase each entity matched, descending, then id. Logged at `store.match_entities` with counts only.

**`find_live_relationship`** — `SELECT * FROM relationship WITH INDEX relationship_live WHERE owner.team = $team AND from_ent = $from AND kind = $kind AND to_ent = $to AND valid_to IS NONE AND <novis read filter> LIMIT 1` (`$team` = `ctx.teams[0]`, or `NONE` for a user-private write). **`relationship_kinds`** — the distinct `kind` values of the caller's readable relationships, at most `limit`, by most recent `ingested_at`. **`link_fact`** — `UPSERT relationship_fact:[$rel, $fact, $role] CONTENT {…, at: now}`. **`links_of_fact`** — the relationships with a `relationship_fact` row for the fact, with that row's role, read-filtered.

**11′. `end_validity` (adds to step 11).** After the fact's `valid_to` is set, in the same transaction: for each relationship with a row `(rel, fact, asserted)` and `valid_to IS NONE`, if no other fact with role `asserted` on it is currently valid, `UPDATE relationship SET valid_to = $at`. Logged `store.links_closed { count }`.

**12′. `supersede` (adds to step 12).** As 11′ for `old_id` at `at`, after the `new` fact's own links are already recorded (C4 writes links before it supersedes), so a link the new fact repeats stays live.

**13′. `hard_delete` (adds to steps 13–14).** The removal set's facts (the fact and each derived insight) also remove their `relationship_fact` rows; every relationship that then has no row of role `asserted` and no `replaces` is deleted with them, in the same `DELETE`. Its id joins the sorted id list the digest is computed over and `DeletionProof.relationships_removed`. The re-read check also re-reads each removed relationship id.

**19d′. `merge_entities` (adds to step 19d).** In the same transaction: recompute `keep_id.names_norm` from the folded aliases; repoint relationships; then, for any two live relationships that became identical `(from_ent, kind, to_ent)`, keep the one with the earlier `valid_from` (ties: smaller id), move the other's `relationship_fact` rows onto it, and delete the other. A relationship that became a self-loop (`from_ent = to_ent`) is deleted.

**20d. `list_entities(ctx, search, cursor, limit)`.** `limit` 1..=100. Read-filtered entities ordered by `canonical_name`, then id, keyset-paged by `(canonical_name, id)` encoded base64url; `search` (1..=200 chars) matches when its normalised form is a substring of any `names_norm` element. For each entity, `fact_count` = currently-valid readable facts naming it, `link_count` = live readable relationships touching it (two grouped counts over the page's ids, never one query per entity).

**20e. `entity_detail(ctx, id, limit)`.** `limit` 1..=200. The entity (read-filtered, else `Ok(None)`); its currently-valid readable facts, newest `ingested_at` first; its relationships live and ended, newest `valid_from` first; `names` maps every link endpoint id to its canonical name.

**20f. `fact_versions(ctx, id, limit)`.** `limit` 1..=50. From the fact, follow `supersedes` back and `superseded_by` forward, each hop read-filtered; stop at `limit` or a missing or unreadable link. Return oldest first.

**20g. `keep_fact(ctx, id, salience_floor)`.** Read access via `fact_read_filter` (else `NotFound`); `valid_to` set → `NotFound`; `UPDATE fact SET memory_class = 'semantic', salience = math::max([salience, $floor])`. Idempotent. Logged at `store.keep_fact`.

**20h. `count_entity(ctx, id)`.** Readable entity (else `NotFound`); `facts` = readable facts naming it, any validity; `links` = readable relationships touching it, any validity.

**20i. `erase_entity(ctx, id)`.** Readable entity (else `NotFound`). Collect every fact naming it (any validity, `fact_read_filter`), every consolidated fact derived from those, and every relationship touching it. In one transaction delete those facts, their `relationship_fact` rows, those relationships, any other relationship left with no asserting fact (13′), and the entity. Re-count: any fact naming the entity, or the entity itself, remaining → `StoreError::PartialDelete`. Digest: SHA-256 hex over the newline-joined sorted removed ids. Logged at `store.erase_entity` with counts only.

**20j. `delete_relationship(ctx, id)`.** Readable relationship (else `NotFound`); delete it and its `relationship_fact` rows; re-read; build `DeletionProof { record_id: id, derived_removed: [], embeddings_removed: 0, relationships_removed: [id], digest }`.

**20k. `backfill_entity_names(tenant, batch)`.** Select up to `batch` entities with `names_norm IS NONE`, compute and write `names_norm`, return the count; when it returns 0, set `maintenance_state.names_backfilled_at = now`. Called by the embedding caller at start in a loop (ordin: the Memory Service's start step) and by the C7 cycle while unset.

**20l. Team import and export (ordin M-7, via C9).** `put_relationship` stays an upsert by id (M-7 already requires it of the SQL storage) so an imported relationship replaces a stored one of the same id; C9's `import_facts` then calls `link_fact(.., Asserted)` for each imported fact naming both endpoints, and its `export_scope` reads relationships for a page with one query joining `relationship_fact` on the page's fact ids (either role, any validity), read-filtered. No other store method is added for them.

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

-- 0008_graph.down.surql
REMOVE FIELD IF EXISTS names_backfilled_at ON maintenance_state;
REMOVE INDEX IF EXISTS entity_names ON entity;
REMOVE FIELD IF EXISTS names_norm ON entity;
REMOVE TABLE IF EXISTS relationship_fact;
REMOVE INDEX IF EXISTS relationship_live ON relationship;
REMOVE FIELD IF EXISTS replaces ON relationship;
```

Rollback (sql-safety): the up path adds optional fields, two indexes and one table; no row is changed and nothing narrows. The down path is **destructive for `relationship_fact`**: removing the table drops the record of which facts asserted each link, after which links no longer close when their facts end (SA-GRAPH-04) and erasure no longer removes them; restoring needs the pre-migration backup or a re-write of the facts. `names_norm` values stay on SCHEMALESS entity rows but are unindexed. Forward-only in practice; the down path exists for the test harness. The SQL storage equivalent (`mem_relationship.replaces`, `mem_rel_live`, `mem_relationship_fact`, `mem_entity_name`, `mem_maintenance_state.names_backfilled_at`) is owned by ordin's Memory Service spec (SA-COLL-05 applies to it unchanged).

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
- **Observability.** `store.recall` adds `graph_seeds`, `graph_reached`; new log events `store.neighbours`, `store.match_entities`, `store.links_closed`, `store.erase_entity`, `store.keep_fact`, `store.recall_graph_unavailable`; metric `recall_graph_reached` (histogram, labelled `{tenant}`).

##### Gaps

None.
