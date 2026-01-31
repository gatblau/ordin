### SPEC: Write Pipeline

**File:** `src/write_pipeline` | **Package:** `memory::write_pipeline` | **Phase:** 3 | **Dependencies:** C1 (Memory Store), C2 (Durable Work Queue)

> **Mode:** greenfield · amended 2026-01-21 (SA-PII-02, SA-QUEUE-03, SA-WRITE-STRUCTURED-02) · amended January 2026 (ADR-022: triples, change, end, conflict; Approved by the user January 2026) · amended January 2026 (SA-TRIPLE-15, step 8″.6; Approved by the user January 2026)
> **derivedFromHld:** 0.13.0

#### Purpose

The Write Pipeline is the asynchronous consumer of `ExtractFact` work jobs from the durable work queue. It turns a structured, agent-asserted `remember` payload into a clean, scoped, provenance-tagged Fact in the Memory Store, or into a quarantined / rejected record when the content cannot be trusted. For each claimed job it runs eight ordered steps — noise filtering, structured-content intake, normalisation, entity resolution, scoring, PII scanning, the write gate, and embedding-then-persist. The steps are numbered by role: entity resolution (step 4) runs after the write gate and on the admit path only, so a write the gate rejects or holds creates no entity (*Internal Logic* states the run order). It never runs on the read path (ADR-004): a slow or failed write is retried with backoff behind the queue and is dead-lettered after the attempt cap, and a write never blocks a read. It is the component that enforces ADR-008 (write-gate trust validation and untrusted-data separation), SA-PII-01 (PII redaction / flagging), and the data-minimisation obligation (store structured assertions, drop low-signal noise).

#### Approach

The component is a **linear step pipeline driven by a claim/lease loop**, not a state machine and not a per-step actor graph. One async task per worker enumerates tenants (`MemoryStore::list_tenants`) and calls `WorkQueue::claim(tenant, &[JobKind::ExtractFact], lease)` across them round-robin (RISK-016 — the tenant is named, not inherited from connection state); each claimed job flows through the step functions in the fixed run order of *Internal Logic*, and the first step that decides the job is finished (filtered as noise, rejected by the gate) short-circuits to `WorkQueue::complete`. This was chosen over a state-machine-per-job (rejected — the steps are a fixed acyclic sequence with no re-entry, so a state machine adds bookkeeping with no branching benefit) and over fanning each step to its own queue stage (rejected — every step shares the same job context and there is no independent scaling need across steps; multi-stage queuing would multiply queue round-trips against SA-LAT budgets for no gain). Entity resolution is a **three-tier escalation** (deterministic rules → ML similarity → create-new) so the cheap tiers settle the common cases and a genuinely ambiguous identity yields a fresh entity rather than risking a wrong merge; recall holds no LLM (ADR-015), so create-new is the terminal tier by design (not a deferral). The component owns only the `quarantine` table; the `fact` and `entity` tables are owned by C1 and written through the `MemoryStore` trait.

#### Shared Context

Duplicated from Phase 2 (binding). A reader implements from this section alone.

**Domain types (2C.2):**

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Fact {
    pub id: String,                       // "fact:<uuidv7>" (SA-ID-01)
    pub content: serde_json::Value,       // structured assertion (object), not free text
    pub entities: Vec<String>,            // entity ids this fact connects (>=1)
    pub source_id: Option<String>,        // "source:<uuidv7>" provenance, null for agent-stated
    pub memory_class: MemoryClass,
    pub visibility: Visibility,
    pub owner: ScopeRef,                  // owning (tenant, team, user)
    pub valid_from: DateTime<Utc>,        // RFC3339 ms (SA-TIME-01)
    pub valid_to: Option<DateTime<Utc>>,  // null = currently true (open interval)
    pub ingested_at: DateTime<Utc>,       // server-set
    pub confidence: f64,                  // [0,1] (SA-SCORE-01)
    pub salience: f64,                    // [0,1]
    pub stability: f64,                   // decay stability `s` (SA-DECAY-01), >=0.0
    pub pii_review: bool,                 // true => low-confidence PII flagged for review (SA-PII-01); set in step 8
    pub supersedes: Option<String>,       // fact id this one replaces
    pub superseded_by: Option<String>,    // fact id replacing this one
    pub derived_from: Vec<String>,        // source fact ids (consolidated insights only)
    pub last_recalled_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Entity {
    pub id: String,                       // "entity:<uuidv7>"
    pub canonical_name: String,           // 1..=512 chars, non-empty
    pub aliases: Vec<String>,
    pub owner: ScopeRef,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Source {                       // provenance; upserted in step 8, trust read in steps 5/7
    pub id: String,                       // "source:<uuidv7>"
    pub origin_ref: String,               // document/system reference, opaque to recall
    pub modification_marker: Option<String>, // ETag / Last-Modified token for freshness (C5)
    pub trust_signal: f64,                // [0,1] prior trust of this source (MEMORY_SOURCE_TRUST_DEFAULT if new)
    pub owner: ScopeRef,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryClass { Episodic, Semantic, Consolidated }   // procedural rejected (SA-CLASS-01)

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility { UserPrivate, TeamShared, TenantShared } // (SA-VIS-01)
```

**Scope types (2C.3):**

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ScopeRef {
    pub tenant: String,            // tenant id -> SurrealDB namespace (ADR-011)
    pub team: Option<String>,      // team id, null for user-only facts
    pub user: String,              // user id, bound to the OIDC subject claim
}

#[derive(Clone)]
pub struct ScopeContext {
    pub tenant: String,
    pub teams: Vec<String>,
    pub user: String,
    pub token_jti: String,
    pub allowed_ops: OpSet,
    pub correlation_id: String,
}

#[derive(Clone, Copy)]
pub struct OpSet { pub read: bool, pub write: bool, pub forget: bool }
```

> The Write Pipeline runs **after** authorisation; it consumes a `WorkJob` whose `scope: ScopeRef` was derived by C3 at enqueue time and is carried on the job. It constructs an internal `ScopeContext` for `MemoryStore` calls from the job's `ScopeRef` (`tenant`, `user`, `team` → single-element `teams`) with `correlation_id` taken from the job and `allowed_ops = { read: true, write: true, forget: false }`. The pipeline never reads scope from request-body content.

**Work-queue types (2C.5):**

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct WorkJob {
    pub id: String,                          // "work_job:<uuidv7>"
    pub kind: JobKind,
    pub payload: serde_json::Value,          // kind-specific, validated by the consumer
    pub scope: ScopeRef,
    pub idempotency_key: Option<String>,
    pub attempts: u32,
    pub status: JobStatus,
    pub not_before: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub leased_until: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobKind { ExtractFact, ReEmbedFact, HardDelete }  // ReReadSource removed by ADR-014; Consolidate removed by ADR-015

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus { Pending, Leased, Done, DeadLetter }
```

**Traits consumed (2C.6) — exact signatures:**

```rust
#[async_trait]
pub trait MemoryStore: Send + Sync {                 // impl: embedded SurrealDB (C1); full surface in C1/§2C.6
    async fn put_fact(&self, f: &Fact) -> Result<(), StoreError>;          // persists the fact + embedding
    async fn find_entity_by_name(&self, ctx: &ScopeContext, name: &str)    // entity-resolution lookup (step 4)
        -> Result<Vec<Entity>, StoreError>;
    async fn put_entity(&self, e: &Entity) -> Result<(), StoreError>;      // upsert (create-new / update, step 4)
    async fn merge_entities(&self, ctx: &ScopeContext, keep_id: &str, merge_id: &str)
        -> Result<(), StoreError>;                                         // fold a duplicate into a kept entity
    async fn put_source(&self, s: &Source) -> Result<(), StoreError>;      // upsert provenance (step 8)
    async fn get_source(&self, ctx: &ScopeContext, id: &str)              // read source trust (steps 5,7)
        -> Result<Option<Source>, StoreError>;
}

#[async_trait]
pub trait WorkQueue: Send + Sync {                    // impl: store-backed (C2)
    async fn claim(&self, kinds: &[JobKind], lease: Duration)
        -> Result<Option<WorkJob>, QueueError>;
    async fn complete(&self, job_id: &str) -> Result<(), QueueError>;
    async fn fail(&self, job_id: &str, retryable: bool) -> Result<(), QueueError>;
}

#[async_trait]
pub trait EmbeddingClient: Send + Sync {              // impl: HTTP adapter
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError>; // dim = config
}

#[async_trait]
pub trait PiiDetector: Send + Sync {                  // impl: in-process deterministic detector
    async fn scan(&self, content: &serde_json::Value) -> Result<Vec<PiiSpan>, ProviderError>;
}
```

`StoreError`, `QueueError`, and `ProviderError` are typed errors owned by C1 / C2 / the provider adapters and referenced here by name. `ExtractedFact`, `EntityMention`, and `PiiSpan` are defined in `src/types/ports.rs` (the §2C.6 trait surface, because they cross the `PiiDetector` trait boundary) and are restated in full in this component's *Public Interface*.

**Provider wire contracts (as built in `src/providers/mod.rs`).** The only network provider this component consumes is the embedding provider — a thin `reqwest` POST adapter with no domain logic; the concrete JSON request / response shape below is the wire contract the integration-suite wiremock stubs honour. It POSTs with `Content-Type: application/json` and, where an API key is configured, `Authorization: Bearer <key>`; the call carries a uniform 10 s timeout. A `reqwest` timeout maps to `ProviderError::Timeout`, a non-2xx status to `ProviderError::Status(code)`, a transport failure to `ProviderError::Transport`, and a malformed body to `ProviderError::Malformed`.

- **Embedding** — `POST {MEMORY_EMBED_URL}/embeddings`
  - Request: `{ "model": "<MEMORY_EMBED_MODEL_VERSION>", "input": ["text", ...] }`
  - Response: `{ "embeddings": [[f32, ...], ...] }` — one vector per input, each of `MEMORY_EMBED_DIM`.

**PII detection (in-process, ADR-015).** PII detection is an **in-process deterministic detector** (the default `PiiDetector` impl) — regex/pattern matching for structured identifiers (email, phone, card-via-Luhn, IP, national-id), confidence by pattern strength; no network call, no endpoint or key (ADR-015). The `PiiDetector` trait remains a DI seam so a model-backed detector can be injected if a deployment needs one. There is no PII wire contract: the default detector never leaves the process.

**Config keys used by this component (2D), with the env var, type, and default:**

| Variable | Type | Default | Description |
|---|---|---|---|
| `MEMORY_TRUST_ADMIT` | f64 | `0.7` | Write-gate admit threshold (SA-WGATE-01). A trust score `>=` this admits. |
| `MEMORY_TRUST_QUARANTINE` | f64 | `0.4` | Write-gate quarantine threshold. `quarantine <= score < admit` quarantines; `< quarantine` rejects. |
| `MEMORY_PII_REDACT_CONF` | f64 | `0.9` | PII redaction confidence threshold (SA-PII-01). Span confidence `>=` this is redacted; below sets `pii_review=true`. |
| `MEMORY_EMBED_DIM` | u32 | `1024` | Embedding dimension; the embed vector length must equal this (SA-EMBED-01). |
| `MEMORY_JOB_MAX_ATTEMPTS` | u32 | `5` | Retry cap before dead-letter (SA-QUEUE-02). Owned by C2; read here to decide retryable vs terminal failure. |

**Binding assumptions duplicated from Phase 1:**

- **SA-SCORE-01** — `confidence` and `salience` are `f64` in the closed interval `[0.0, 1.0]`, validated at write.
- **SA-WGATE-01** — trust in `[0,1]`; `>= trust_admit` persist; `trust_quarantine <= score < trust_admit` quarantine; `< trust_quarantine` reject; instruction-like content capped below `trust_quarantine`.
- **SA-PII-01** — span confidence `>= pii_redact` is replaced in place by the literal token `‹redacted:‹type››`; a lower-confidence flag stores the fact with `pii_review=true`.
- **SA-QUEUE-02** — failed job retried with exponential backoff (base 2 s, factor 2, jitter ±20 %, max 5 attempts) then moved to dead-letter. Backoff scheduling and dead-letter transition are owned by C2; this component signals retryable vs terminal via `WorkQueue::fail`.
- **SA-TIME-01** — timestamps are UTC RFC 3339 with millisecond precision; `valid_from` required, `valid_to` nullable, `ingested_at` server-set.
- **SA-ID-01** — server-generated UUIDv7 rendered lowercase; record id is `<table>:<uuid>`.
- **SA-CLASS-01** — `memory_class` is one of `episodic`, `semantic`, `consolidated`; `procedural` is rejected (`VAL_UNSUPPORTED_CLASS`).

#### Public Interface

The component exposes one runtime entrypoint (the consumer loop), `process`, and a set of pure / async step functions. The step functions and `process` are declared `pub` (not `pub(crate)`) so the unit-test module and the integration suite can drive each step directly.

**Types defined by this component:**

```rust
/// The internal pre-persistence fact shape. One per asserted fact.
/// This is NOT a stored Fact: it carries no id, no scores, no validity, no scope —
/// those are assigned by later pipeline steps. Defined in `src/types/ports.rs`
/// (the §2C.6 trait surface). Produced internally by `intake` from the structured
/// agent-asserted content; memory runs no extractor (ADR-015).
#[derive(Clone, Serialize, Deserialize)]
pub struct ExtractedFact {
    pub content: serde_json::Value,          // structured assertion (JSON object), not free text
    pub entities: Vec<EntityMention>,        // raw entity references to resolve (>=1)
    pub memory_class: MemoryClass,           // proposed class; procedural is impossible (enum)
    pub confidence: f64,                     // [0,1] intake confidence (1.0 for agent-asserted content)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EntityMention {
    pub surface: String,                     // 1..=512 chars, the text as it appeared
    pub canonical_name: Option<String>,      // optional coarse type hint, e.g. "person", "team"
}

/// A PII span returned by PiiDetector::scan, defined in `src/types/ports.rs`.
/// (Produced by the injected PiiDetector trait; consumed only by this component.)
#[derive(Clone, Serialize, Deserialize)]
pub struct PiiSpan {
    pub json_pointer: String,                // RFC 6901 pointer into content locating the string value
    pub start: u32,                          // byte offset within the located string value (inclusive start)
    pub end: u32,                            // exclusive byte offset; end > start
    pub pii_type: String,                    // e.g. "email", "phone", "national_id"
    pub confidence: f64,                     // [0,1]
}

/// The outcome of one job, used for logging, metrics, and tests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
/// `Erased`: an erasure removed the job while it ran; nothing it wrote stays and there is no job to complete.
pub enum WriteOutcome { Persisted, Quarantined, Rejected, FilteredNoise, Erased }

/// Decision returned by the write gate, separating the three audited outcomes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateDecision { Admit, Quarantine, Reject }

/// Result of the imperative-pattern detector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InstructionLikelihood {
    pub is_instruction_like: bool,           // true => trust score capped below trust_quarantine
    pub matched_patterns: u32,               // count of imperative patterns matched (for audit)
}
```

**Runtime entrypoint and step functions:**

```rust
pub struct WritePipeline {
    store: Arc<dyn MemoryStore>,
    queue: Arc<dyn WorkQueue>,
    embed: Arc<dyn EmbeddingClient>,
    pii: Arc<dyn PiiDetector>,
    // Shared SurrealDB handle for the C4-owned `quarantine` table (the quarantine sink); `None`
    // disables quarantining (the gate then surfaces a defended `quarantine sink not wired` error).
    db: Option<Surreal<Db>>,
    cfg: WritePipelineConfig,
}

// `Clone`, not `Copy`: it carries `embed_model_version: String`, stored alongside the embedding.
#[derive(Clone)]
pub struct WritePipelineConfig {
    pub trust_admit: f64,           // MEMORY_TRUST_ADMIT
    pub trust_quarantine: f64,      // MEMORY_TRUST_QUARANTINE
    pub pii_redact_conf: f64,       // MEMORY_PII_REDACT_CONF
    pub embed_dim: u32,             // MEMORY_EMBED_DIM
    pub max_attempts: u32,          // MEMORY_JOB_MAX_ATTEMPTS
    pub source_trust_default: f64,  // MEMORY_SOURCE_TRUST_DEFAULT (prior trust for a newly-seen source)
    pub embed_model_version: String,// MEMORY_EMBED_MODEL_VERSION (stored with the embedding for re-embed bookkeeping)
    pub claim_lease: Duration,      // claim lease length (default 30 s)
    pub per_job_budget: Duration,   // per-job wall-clock budget (default 30 s)
}

impl WritePipeline {
    /// Construct with injected dependencies and validated config. `db` is the shared store connection
    /// used for the C4-owned `quarantine` table writes.
    pub fn new(
        store: Arc<dyn MemoryStore>,
        queue: Arc<dyn WorkQueue>,
        embed: Arc<dyn EmbeddingClient>,
        pii: Arc<dyn PiiDetector>,
        db: Surreal<Db>,
        cfg: WritePipelineConfig,
    ) -> Self;

    /// The consumer loop. Claims ExtractFact jobs and processes each to a terminal
    /// queue state. Runs until `cancel` is triggered. Never returns on transient
    /// queue errors — it logs and re-claims after a short delay.
    pub async fn run(&self, cancel: CancellationToken) -> ();

    /// Process exactly one claimed job to a terminal queue state (complete or fail).
    /// Returns the outcome for metrics/tests. Bounded by cfg.per_job_budget.
    pub async fn process(&self, ctx: &ScopeContext, job: &WorkJob)
        -> Result<WriteOutcome, AppError>;

    // ---- step functions (ordered) ----

    /// Step 1. True => content is empty or low-signal and the job is completed as noise.
    pub fn filter_noise(&self, content: &serde_json::Value) -> bool;

    /// Step 2. Wrap the structured agent-asserted content directly as a single
    /// ExtractedFact. No provider call (ADR-015 — recall holds no LLM).
    pub async fn intake(&self, content: &serde_json::Value, memory_class: Option<MemoryClass>)
        -> Result<Vec<ExtractedFact>, AppError>;

    /// Step 3. Canonicalise content (key order, whitespace, unicode NFC — currently identity, see Internal Logic).
    pub fn normalise(&self, ef: &ExtractedFact) -> ExtractedFact;

    /// Step 4. Resolve each mention to an entity id via rules -> ML -> create-new escalation,
    /// creating entities through MemoryStore. Never silently overwrites.
    pub async fn resolve_entities(&self, ctx: &ScopeContext, ef: &ExtractedFact)
        -> Result<Vec<String>, AppError>;

    /// Step 5. Score salience and confidence into [0,1].
    pub fn score(&self, ef: &ExtractedFact, source_trust: f64) -> (f64 /*salience*/, f64 /*confidence*/);

    /// Step 6. PII scan; redact high-confidence spans in place, return (content, pii_review).
    pub async fn pii_scan(&self, content: &serde_json::Value)
        -> Result<(serde_json::Value, bool /*pii_review*/), AppError>;

    /// Step 7a. Detect imperative/instruction-like content.
    pub fn detect_instruction(&self, content: &serde_json::Value) -> InstructionLikelihood;

    /// Step 7b. The write gate. Compute trust in [0,1] and map to a three-way decision.
    pub fn write_gate(&self, confidence: f64, source_trust: f64, instr: InstructionLikelihood)
        -> GateDecision;

    /// Step 8. Embed fact content and persist Fact + vector via MemoryStore::put_fact then set_fact_embedding.
    pub async fn embed_and_persist(&self, ctx: &ScopeContext, fact: &Fact)
        -> Result<(), AppError>;

    /// Persist a gate-quarantined record into the `quarantine` table (not the `fact` table).
    pub async fn quarantine(&self, ctx: &ScopeContext, qr: &QuarantineRecord)
        -> Result<(), AppError>;
}
```

> The component produces `AppError` (2C.7); `process` maps each internal failure to one `AppError` variant, and the consumer loop translates that variant into `WorkQueue::fail(retryable)` (transient → `true`, terminal → `false`) or `WorkQueue::complete` (terminal success / noise / reject / quarantine). There is no HTTP surface — the HTTP contract for `remember` belongs to C8; this component never returns a status code to a caller.

##### Example

**Input** — a claimed `ExtractFact` job (payload is the `remember` request body the API enqueued):

```json
{
  "id": "work_job:018f3a2b-7c41-7e90-9d22-1a2b3c4d5e6f",
  "kind": "extract_fact",
  "payload": {
    "content": { "subject": "Team Alpha", "predicate": "owns", "object": "the orders table" },
    "source": { "origin_ref": "doc://wiki/ownership", "modification_marker": "W/\"abc\"" }
  },
  "scope": { "tenant": "acme", "team": "platform", "user": "u-77" },
  "idempotency_key": "ik-001",
  "attempts": 0,
  "status": "leased",
  "not_before": "2026-01-10T10:00:00.000Z",
  "created_at": "2026-01-10T10:00:00.000Z",
  "leased_until": "2026-01-10T10:00:30.000Z"
}
```

**Output** — a persisted Fact (via `MemoryStore::put_fact`), trust score 0.86 ≥ `trust_admit` 0.7:

```json
{
  "id": "fact:018f3a2b-9001-7aaa-8bbb-0c1d2e3f4051",
  "content": { "subject": "entity:018f...alpha", "predicate": "owns", "object": "entity:018f...orders" },
  "entities": ["entity:018f...alpha", "entity:018f...orders"],
  "source_id": "source:018f...ownership",
  "memory_class": "semantic",
  "visibility": "user-private",
  "owner": { "tenant": "acme", "team": "platform", "user": "u-77" },
  "valid_from": "2026-01-10T10:00:00.123Z",
  "valid_to": null,
  "ingested_at": "2026-01-10T10:00:00.123Z",
  "confidence": 0.86,
  "salience": 0.62,
  "stability": 1.0,
  "pii_review": false,
  "supersedes": null,
  "superseded_by": null,
  "derived_from": [],
  "last_recalled_at": null
}
```

The job is then marked `complete`; `WriteOutcome::Persisted`.

#### Internal Logic

`process(ctx, job)` runs the numbered steps below in this order: step 0; steps 1 and 2; the `Source` upsert; then, per `ExtractedFact`, steps 3, 6′ and 7′ (the stated names and kinds), 5, 6, 7 and the origin re-check; and on the admit path steps 4, 8 and 8′, the re-reads after persist, and step 4′ last. Each step states what it does, which dependency it calls, which errors it can raise, and what it logs. All logs are structured and carry `correlation_id` (from `job.scope`/`ctx`), `job_id`, and `tenant`; content values and PII are never logged.

0. **Validate job payload.** Deserialise `job.payload` into `{ content: Value, source: Option<SourceInput>, memory_class: Option<MemoryClass>, fact_class: Option<String> }`. `fact_class` is the caller-owned class C9 **already authorised** against the token's `memory_fact_authority` claim (SA-AUTHZ-01); C4 carries it to storage and judges nothing about it — it validates no vocabulary, compares it against nothing, and treats no value specially (SA-AUTHZ-09, SA-LLMFREE-01). A job that carries none is an unclassed write and stays unclassed. If `job.kind != ExtractFact`, raise `AppError::Validation` (terminal — wrong consumer claimed it; this is impossible under the claim filter but defended). If `content` is absent or not a JSON object, raise `AppError::Validation` mapped to `VAL_INVALID_BODY` (terminal — the payload must carry a structured assertion object). Build the internal `ScopeContext` from `job.scope`. Logs: `write.job.claimed` with `job_id`, `tenant`, `attempts`. Then `StoragePort::job_is_live(tenant, job.id)`: `false` means an erasure removed the job since it was claimed, so log `write.erased_in_flight` and return `WriteOutcome::Erased` with no `WorkQueue::complete` call, there being no job left to complete.

1. **Filter noise** (`filter_noise`). Drop the job as noise when, after trimming, the serialised `content` is empty, is an empty object/array, or its total non-whitespace string length across all string values is `< 3` characters. On noise: log `write.filtered_noise`, call `WorkQueue::complete(job.id)`, return `WriteOutcome::FilteredNoise`. Errors: none (pure). This is the data-minimisation step (HLD 06).

2. **Intake** (`intake`). Wrap the structured agent-asserted `content` as a single `ExtractedFact` with `entities` derived from the content's referenced entities (a deterministic `scan_entity_mentions` scan over the well-known `subject`/`object` string fields), `memory_class` taken from the request's `memory_class` (else `Episodic`), and `confidence = 1.0` (the caller asserts it directly). **No LLM call** (ADR-015 — recall holds no LLM); there is no provider-error path here. If the wrapped content yields no salient assertion (empty after intake), complete the job as `FilteredNoise`. Logs: `write.intake` with `fact_count`. **Security:** the content is treated as data, never executed or interpreted as instructions to the pipeline.

   **`Source` upsert.** After step 2 and ahead of every check, once per job, `upsert_source` writes the `Source` when `source` was provided, so a write the gate then rejects still leaves its `Source` row. The id is `source_id(owner, origin_ref)`: `source:<UUIDv5(NAMESPACE_DNS, seed)>`, where `seed` is the JSON array `["source", tenant, team, user, origin_ref]` of the owner's fields, so one origin cited by two owners has two rows, each with its own `trust_signal`, and nothing one account cites reaches another's. `owner` is the job's tenant, the first team of `ctx.teams` and the user. An existing row keeps its `trust_signal` and `collection`; a new one takes `MEMORY_SOURCE_TRUST_DEFAULT` and the write's collection. `source_trust` for steps 5 and 7 is then the row's `trust_signal`, or `1.0` for a source-less write.

   The remaining steps then run **once per `ExtractedFact`** in the returned vector; each produces its own gate decision and persistence outcome. The job reports the strongest outcome (`Persisted` over `Quarantined` over `Rejected` over `FilteredNoise`), and `WorkQueue::complete` runs once, after the last fact; `Erased` returns at once with no `complete`. A failure on one fact aborts the whole job as retryable (the queue replays all facts; persistence is idempotent on the derived fact id — see step 8).

3. **Normalise** (`normalise`). Canonicalise the `ExtractedFact.content`: sort object keys (recursively, lexicographic), collapse internal whitespace runs to single spaces and trim string values, and apply Unicode NFC normalisation; each `EntityMention.surface` is whitespace-collapsed the same way. Pure; no dependency. Errors: none. Logs: none (per-fact, high volume). **Assumption (NFC):** the as-built `nfc` helper is the identity function — input is expected to arrive already NFC-composed, so the canonicalisation that affects recall is key order plus whitespace; full Unicode decomposition / recomposition is deferred to avoid a heavy dependency. This is a documented assumption, not a gap.

4. **Entity-resolve** (`resolve_entities`) — runs after step 7, on the admit path only, over the redacted content: a write the gate rejects or holds creates no entity, and no entity takes its name from an unscreened value. Three-tier escalation, returning `>=1` entity ids. For each `EntityMention`: (a) **Rules tier** — normalise the `surface` (NFC, lowercase, trim, collapse whitespace, strip leading/trailing ASCII punctuation; a normalised-empty mention is skipped) and look up an existing `Entity` by exact canonical-name or alias match within `ctx` via `MemoryStore::find_entity_by_name`. When the write's visibility is not `UserPrivate`, an entity matched through a name in its `private_aliases` is not a hit: a shared write must not land on an entity through a name only a private fact gave it, because the entity's other names would then reach the write's readers. On a unique hit, resolve to it; on **multiple** exact hits, deterministically pick the lexicographically-smallest entity id rather than guess (a later maintenance merge folds the duplicates, C7). (b) **ML tier** — on no exact hit, run a second `find_entity_by_name` against the *raw* surface form (a case / punctuation variant) and score each candidate's canonical name against the normalised surface with a character-bigram Sørensen–Dice coefficient; a single candidate at or above the similarity admit threshold (`ML_SIMILARITY_ADMIT = 0.92`) resolves, otherwise escalate. (c) **create-new tier (v1)** — on ambiguity, the mention is resolved by creating a **new** `Entity` via `MemoryStore::put_entity` (canonical_name = normalised surface form, the original surface form added to `aliases`, `owner` = the job's tenant, the first team of `ctx.teams` and the user, whatever the fact's visibility: team and folder erasure find rows by that team, and the read filter, not the owner, decides who sees the name) — never merging into an existing entity without an above-threshold match, and **never silently overwriting** an existing entity (HLD 04). The HLD's third tier named LLM adjudication, but memory has no LLM (ADR-015), so create-new is the terminal tier by design (not a deferral). Duplicate entities created this way are folded later by the C7 Maintenance Worker via `MemoryStore::merge_entities`. If, after processing every mention, no entity id was produced (the extractor offered no usable mention), the pipeline synthesises a single anchor entity from the content (a normalised `subject` string, else the first non-empty string leaf; a value step 6 redacted is passed over while another value remains) so a salient assertion is still anchored rather than dropped; only when that synthesis also yields an empty name does it raise `AppError::Validation` (terminal — a fact must connect `>=1` entity, 2C.2). Errors: `StoreError` → `AppError::Store` (retryable). Logs: `write.entities.resolved` with `entity_count`, `created_count`.

5. **Score** (`score`). Compute `salience` and `confidence`, each clamped to `[0,1]` (SA-SCORE-01). `confidence = clamp01(ef.confidence * source_trust_factor)` where `source_trust_factor = 0.5 + 0.5 * source_trust.clamp(0,1)` (a source-less agent assertion → `source_trust = 1.0`). `salience` is a bounded heuristic in `[0,1]` from content richness: base `0.3`, `+0.15` per entity mention (mention weight capped at `0.45`), `+0.2` when the content names a `predicate` / `relation` / `verb` field. Pure. Errors: none. Logs: none.

6. **PII scan** (`pii_scan`). Call `PiiDetector::scan(&view)` → `Vec<PiiSpan>` once, where `view` (SA-PII-02) is `content` with every number written as a string, so a span's `json_pointer` still locates its leaf, and, on an object, one added member under a reserved key not otherwise present, listing the distinct object keys of every depth. For each span with `confidence >= cfg.pii_redact_conf`: locate the string in `view` via `json_pointer`, replace bytes `[start,end)` with the literal `‹redacted:‹{pii_type}››` (SA-PII-01) and write the result to that leaf, which turns a number holding a span into a string. A span in the key list renames that key wherever it occurs; two keys of one object redacted to the same text keep apart by a ` #<n>` suffix. If any span has `confidence < cfg.pii_redact_conf`, set `pii_review = true`. Return `(redacted_content, pii_review)`. Errors: `ProviderError` → `AppError::Provider` (retryable iff timeout/5xx). Logs: `write.pii.scanned` with `redacted_count`, `flagged_count` — never the span text or type values beyond counts.

7. **Write gate** (`detect_instruction` then `write_gate`) — ADR-008 / SA-WGATE-01. (a) `detect_instruction` runs the **imperative-pattern detector** over the content's string values: it matches imperative-mood leading verbs and known prompt-injection markers (case-insensitive). It also reads every object key, with `_`, `-` and `.` read as spaces, for the known phrases by the same rule as a name (step 7′, SA-PII-02): a leading verb is not looked for, because `run`, `set` and `output` are ordinary field names. No pattern can match a number, so numbers are not read here. Contract: it returns `InstructionLikelihood { is_instruction_like, matched_patterns }`; `is_instruction_like` is `true` when `matched_patterns >= 1`. (b) `write_gate` computes `trust = clamp01(0.6 * confidence + 0.4 * source_trust)`; if `instr.is_instruction_like`, `trust = min(trust, cfg.trust_quarantine - 0.0001)` (capped strictly below the quarantine floor so instruction-like content can never admit or quarantine). Then: `trust >= cfg.trust_admit` → `Admit`; `cfg.trust_quarantine <= trust < cfg.trust_admit` → `Quarantine`; `trust < cfg.trust_quarantine` → `Reject`. The three outcomes are distinguishable and each is audited (log `write.gate` with `decision`, `trust`, `is_instruction_like`). On `Reject`: persist nothing, `WorkQueue::complete`, return `WriteOutcome::Rejected`. On `Quarantine`: call `quarantine(ctx, qr)` to write a `QuarantineRecord` — carrying `fact_class` alongside the content and scores, so a reviewer sees what the write *claimed to be* and not only what it contained (SA-AUTHZ-09), with `entities` empty because step 4 has not run and the stated names in `entities_asserted` — to the `quarantine` table. Then `job_is_live`: `false` means an erasure overtook the write, so `drop_quarantine` removes the row, `write.erased_in_flight` is logged and `WriteOutcome::Erased` returned. Otherwise `WorkQueue::complete`, return `WriteOutcome::Quarantined`. Errors (quarantine path): `StoreError` → `AppError::Store` (retryable).

8. **Embed and persist** (`embed_and_persist`) — admit path only. Render the (possibly redacted) `content` to a single keyword string via `content_to_text` (mirroring the C1 store so the embedded text matches the indexed text) and call `EmbeddingClient::embed(&[text])`; take the first returned vector (a missing vector raises terminal `VAL_OUT_OF_RANGE`) and assert its length `== cfg.embed_dim` (SA-EMBED-01) else raise `AppError::Validation` (terminal — a dimension mismatch is a config/provider defect, not retryable). Construct the `Fact`: `id` = `fact:<uuid>` derived deterministically (UUIDv5, DNS namespace, over the JSON array `["fact", tenant, team, user, "<idempotency_key>:<fact_index>"]` of `job.scope`, the scope job dedupe uses) when `idempotency_key` is present, so a queue replay reuses the same id (idempotent persist) and two callers using one key never meet, else a fresh UUIDv7; `entities` from step 4; `source_id` from the `Source` upsert when `source` was provided, else `None`; `memory_class` from the `ExtractedFact`; `fact_class` from the payload verbatim (SA-AUTHZ-09) — and on a **superseding** write it is the class of *this* write, never inherited from the superseded fact, because a supersession is a new assertion that passed the gate with its own token (SA-AUTHZ-10); `visibility` from the payload (C9 admits one from the in-process ordin caller alone), else `UserPrivate` (SA-VIS-01 default); `owner = job.scope`; `valid_from = now`; `valid_to = None`; `ingested_at = now`; scores from step 5; `stability = 1.0`; `pii_review` from step 6, carried on the stored fact (the §2C.2 `Fact` and the C1 `fact` table both carry `pii_review`); `supersedes/superseded_by` empty; `derived_from` as *Step 8 (addition)* below; `last_recalled_at = None`. Before `embed_and_persist`, `link_job_fact(job.scope, job.id, fact.id)` records that the job is about to write the fact, so that a delete of a fact whose job then fails or retries can find that job (C2 `job_fact`); a repeat changes nothing. Call `MemoryStore::put_fact(&fact)` then `MemoryStore::set_fact_embedding(ctx, &fact.id, &vector, embed_model_version)` to attach the embedding and the `MEMORY_EMBED_MODEL_VERSION` it was produced with (re-embed bookkeeping for C7). Errors: `StoreError` → `AppError::Store` (retryable, except as *Failure / retry handling* states); `ProviderError` (embed) → `AppError::Provider` (retryable iff timeout/5xx). On success, once the steps of *Internal Logic (additions)* that follow it have run: `WorkQueue::complete`, return `WriteOutcome::Persisted`. Logs: `write.persisted` with `fact_id`, `memory_class`, `confidence`, `salience`, `pii_review`.

**Failure / retry handling (SA-QUEUE-02).** Any step that returns a **retryable** `AppError` (`Store`, `Queue`, timeout/5xx `Provider`) causes `process` to propagate it; the consumer loop then calls `WorkQueue::fail(job.id, retryable = true)`. C2 owns the backoff schedule (base 2 s, factor 2, jitter ±20 %) and moves the job to `dead_letter` once `attempts >= cfg.max_attempts`. Any **terminal** `AppError` (`Validation`, `Store` carrying `StoreError::Validation` or `StoreError::ScopeForbidden`, non-retryable `Provider`, dimension mismatch) causes `WorkQueue::fail(job.id, retryable = false)`, which C2 dead-letters immediately. The pipeline itself never sleeps for backoff and never deletes a job; both are queue responsibilities. A write being slow or failing here never affects a read (ADR-004) — there is no shared lock between this loop and the read path.

**When a job fails for good (SA-QUEUE-03).** `fail_job` is the one way a failed `ExtractFact` job reaches the queue. When the failure will park it (`!retryable`, or `job.attempts >= cfg.max_attempts`), run `pii_scan` over the payload's `content`, `entities`, `links` and `source`, write the results back with `redacted: true` through `WorkQueue::redact_job`, then call `fail`. A failure of the scan or of `redact_job` is logged and `fail` still runs: the queue removes those fields from a payload not marked redacted, so nothing raw is parked. A job going back for a retry keeps its payload, because the retry needs its names unredacted to screen them. `fail_job` is public so that a host driving `process` itself settles a failure the same way.

**First text (SA-WRITE-STRUCTURED-02).** `first_text_too_long(content, names_entity)` is the check C9 runs at intake, on the normalised content, by the names steps 2 and 4 would use. `synth_entity_name` cuts its result to 512 characters: redaction can lengthen a text that passed intake, and the fact, which can be deleted, keeps the whole text.

#### Data Model

This component owns one table: `quarantine`. The `fact` and `entity` tables are owned by **C1 (Memory Store)** and are written only through the `MemoryStore` trait — this spec does **not** redefine their DDL. Fields this component sets on a persisted `Fact` are listed in *Internal Logic* step 8.

The `quarantine` DDL ships inside the **single squashed initial migration** `migrations/0001_init.{up,down}.surql` (version 1, slug `init`), not a separate `0003_quarantine` migration: the five original numbered migrations were folded into one initial migration before first release (greenfield, no shipped database — FU-018). The Migrator selects the tenant namespace (`USE NS <tenant>; USE DB memory`) before running the file, so the table is per-tenant and isolation is structural (ADR-011). Every `DEFINE` uses `IF NOT EXISTS` so the migration is idempotent.

The store is schemaless for the C1 domain tables (SA-MIG-01); `quarantine` is declared **schemafull** because its shape is stable and audit-critical. Per the SurrealDB 3.x adaptation baked into the migration, the arbitrary nested-JSON `content` field is `TYPE object FLEXIBLE` (3.x requires `FLEXIBLE` after `TYPE object` and rejects un-enumerated nested keys on a bare `TYPE object`). Domain ids are stored as `"table:key"` strings throughout (see `src/store/surreal/convert.rs`), so the link-shaped fields `entities` and `source_id` are `array<string>` / `option<string>`, not native record links — matching how the C1 `fact` table stores the same ids.

```sql
-- C4 — Write Pipeline: quarantine (gate-uncertain content awaiting review).
-- Holds gate-uncertain content (trust in [trust_quarantine, trust_admit)) for review.
-- One row per quarantined ExtractedFact; never an admitted or rejected fact.
DEFINE TABLE IF NOT EXISTS quarantine SCHEMAFULL;

DEFINE FIELD IF NOT EXISTS content            ON quarantine TYPE object FLEXIBLE; -- redacted content (PII removed)
DEFINE FIELD IF NOT EXISTS entities           ON quarantine TYPE array<string>;   -- resolved entity ids: empty on a held write, whose names are in entities_asserted; an imported fact's own ids
DEFINE FIELD IF NOT EXISTS source_id          ON quarantine TYPE option<string>;  -- provenance, null for agent-stated
DEFINE FIELD IF NOT EXISTS memory_class       ON quarantine TYPE string
    ASSERT $value IN ['episodic','semantic','consolidated'];
-- migration 0006_quarantine_fact_class.up.surql — additive, forward-only (SA-AUTHZ-09).
-- The caller-owned class the write claimed, carried from the authorised request. No ASSERT: the
-- vocabulary is the caller's and memory validates it against nothing. Required because this table is
-- SCHEMAFULL and would otherwise reject the whole quarantine write, losing the held record entirely.
DEFINE FIELD IF NOT EXISTS fact_class         ON quarantine TYPE option<string>;
DEFINE FIELD IF NOT EXISTS owner              ON quarantine TYPE object;          -- { tenant, team, user }
DEFINE FIELD IF NOT EXISTS owner.tenant       ON quarantine TYPE string;
DEFINE FIELD IF NOT EXISTS owner.team         ON quarantine TYPE option<string>;
DEFINE FIELD IF NOT EXISTS owner.user         ON quarantine TYPE string;
DEFINE FIELD IF NOT EXISTS trust_score        ON quarantine TYPE float
    ASSERT $value >= 0.0 AND $value <= 1.0;                                        -- the gate's trust in [0,1]
DEFINE FIELD IF NOT EXISTS confidence         ON quarantine TYPE float
    ASSERT $value >= 0.0 AND $value <= 1.0;
DEFINE FIELD IF NOT EXISTS salience           ON quarantine TYPE float
    ASSERT $value >= 0.0 AND $value <= 1.0;
DEFINE FIELD IF NOT EXISTS is_instruction_like ON quarantine TYPE bool;           -- imperative-pattern detector result
DEFINE FIELD IF NOT EXISTS pii_review         ON quarantine TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS idempotency_key    ON quarantine TYPE option<string>;  -- scoped_quarantine_key digest of the originating job's key
DEFINE FIELD IF NOT EXISTS reason             ON quarantine TYPE string;          -- audit: why quarantined
DEFINE FIELD IF NOT EXISTS quarantined_at     ON quarantine TYPE datetime;        -- server-set, RFC3339 ms

DEFINE INDEX IF NOT EXISTS quarantine_owner_idx ON quarantine FIELDS owner.user, quarantined_at;
DEFINE INDEX IF NOT EXISTS quarantine_idem_idx  ON quarantine FIELDS idempotency_key UNIQUE;  -- replay-safe
```

The `idempotency_key` column does not hold the caller's key. It holds `scoped_quarantine_key(owner, key)`: the SHA-256 hex of the JSON array `[tenant, team, user, key]`, so the key is unique within the scope job dedupe uses and two callers using one key hold two rows. An imported fact's record takes the same digest of `team:<fact id>`. `put_quarantine` upserts on it, and `drop_quarantine` removes the row it names.

The Rust mirror:

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct QuarantineRecord {
    pub id: String,                       // "quarantine:<uuidv7>"
    pub content: serde_json::Value,       // redacted content (object)
    pub entities: Vec<String>,            // resolved entity ids; empty on a write the gate held, as step 4 has not run
    pub source_id: Option<String>,
    pub memory_class: MemoryClass,
    /// The caller-owned class the write claimed (SA-AUTHZ-09), so a reviewer of a held write sees
    /// what it claimed to be and not only what it contained. Added by migration 0006.
    #[serde(default)]
    pub fact_class: Option<String>,
    pub owner: ScopeRef,
    pub trust_score: f64,                 // [0,1]
    pub confidence: f64,                  // [0,1]
    pub salience: f64,                    // [0,1]
    pub is_instruction_like: bool,
    pub pii_review: bool,
    pub idempotency_key: Option<String>,  // the caller's key; the store writes its scoped_quarantine_key digest
    pub reason: String,
    pub quarantined_at: DateTime<Utc>,
}
```

**Rollback considerations.** The `quarantine` table is dropped by the single squashed down migration `migrations/0001_init.down.surql` (which removes every table the up migration created, including `quarantine`). This is **destructive** — it drops all quarantined records and the two indexes. Because quarantined records are not yet admitted facts, dropping the table loses content that was awaiting review; before applying the down path in any environment holding data, export the table (`SELECT * FROM quarantine`) so pending-review content can be re-quarantined after a re-apply. The down path is safe (no data loss) only on an empty table. Migrations against a shared store are a user action; dry-run first (SA-MIG-01, sql-safety layer rule). Adding fields later is non-destructive (`DEFINE FIELD ...`); removing or narrowing a field's type is destructive and requires the same export-first treatment.

#### Error Table

The Write Pipeline has **no HTTP surface** — these errors are the `AppError` variants `process` raises and the queue action the consumer loop takes. The `Status`/`Code` columns show the registry mapping that **C8** would apply if the same condition surfaced on the synchronous path; on the async path the effect is the queue transition in the rightmost narrative. (The `remember` API itself only returns `202 Accepted` / replayed ack; these conditions are observed post-ack.)

| Condition | Status | Code | Response Body | Queue effect (async) |
|-----------|--------|------|---------------|----------------------|
| Job payload missing `content` or `content` not a JSON object | 400 | `VAL_INVALID_BODY` | `{"error":{"code":"VAL_INVALID_BODY","message":"content must be a JSON object","correlation_id":"<uuid>"}}` | `fail(retryable=false)` → immediate dead-letter |
| Score out of range, or embed vector length != `MEMORY_EMBED_DIM` | 400 | `VAL_OUT_OF_RANGE` | `{"error":{"code":"VAL_OUT_OF_RANGE","message":"value outside permitted range","correlation_id":"<uuid>"}}` | `fail(retryable=false)` → immediate dead-letter |
| `ExtractedFact.memory_class` is procedural (cannot occur via the enum; defended) | 400 | `VAL_UNSUPPORTED_CLASS` | `{"error":{"code":"VAL_UNSUPPORTED_CLASS","message":"procedural memory is not supported","correlation_id":"<uuid>"}}` | `fail(retryable=false)` → immediate dead-letter |
| `MemoryStore` unreachable (put_fact / entity upsert / quarantine) | 503 | `STORE_UNAVAILABLE` | `{"error":{"code":"STORE_UNAVAILABLE","message":"memory store unavailable","correlation_id":"<uuid>"}}` | `fail(retryable=true)` → backoff retry, dead-letter after max attempts |
| `MemoryStore` operation timed out | 504 | `STORE_TIMEOUT` | `{"error":{"code":"STORE_TIMEOUT","message":"memory store timed out","correlation_id":"<uuid>"}}` | `fail(retryable=true)` → backoff retry |
| Embedding provider timed out | 504 | `PROVIDER_TIMEOUT` | `{"error":{"code":"PROVIDER_TIMEOUT","message":"upstream provider timed out","correlation_id":"<uuid>"}}` | `fail(retryable=true)` → backoff retry |
| Embedding provider returned an error (5xx) | 502 | `PROVIDER_ERROR` | `{"error":{"code":"PROVIDER_ERROR","message":"upstream provider error","correlation_id":"<uuid>"}}` | `fail(retryable=true)` → backoff retry |
| Work queue unreachable when completing/failing a job | 503 | `QUEUE_UNAVAILABLE` | `{"error":{"code":"QUEUE_UNAVAILABLE","message":"work queue unavailable","correlation_id":"<uuid>"}}` | loop logs and re-claims; lease expiry returns the job to `pending` |
| Unexpected internal fault (panic-safety boundary, serialisation bug) | 500 | `INTERNAL` | `{"error":{"code":"INTERNAL","message":"internal error","correlation_id":"<uuid>"}}` | `fail(retryable=true)` → backoff retry |

Non-error terminal outcomes (not failures, no error code): **FilteredNoise** and **Rejected** → `WorkQueue::complete`; **Quarantined** → write `QuarantineRecord` then `WorkQueue::complete`; **Erased** → no queue call, the job having gone. These are audited via the `write.gate` / `write.filtered_noise` logs, not via the error registry.

#### Acceptance Criteria (Gherkin)

```gherkin
Feature: Write Pipeline

  Scenario: Happy path — structured content is wrapped, scored, and persisted (no LLM)
    Given an ExtractFact job whose content is a structured assertion object with a trusted source
    And no LLM is wired (recall holds no LLM, ADR-015)
    And the in-process PII detector returns no spans
    When the Write Pipeline processes the job
    Then the content is wrapped as a single ExtractedFact with confidence 1.0
    And the write gate computes a trust score >= MEMORY_TRUST_ADMIT (0.7)
    And the fact is embedded with a vector of length MEMORY_EMBED_DIM
    And MemoryStore::put_fact is called once with confidence and salience in [0,1]
    And the job is completed with outcome Persisted

  Scenario: Edge case — non-object content is rejected as invalid
    Given an ExtractFact job whose content is a JSON string, not an object
    When the Write Pipeline processes the job
    Then process returns AppError mapping to status 400 with code VAL_INVALID_BODY
    And WorkQueue::fail is called with retryable = false

  Scenario: Edge case — empty/low-signal content is filtered as noise
    Given an ExtractFact job whose content has total non-whitespace string length < 3 characters
    When the Write Pipeline processes the job
    Then no extraction, scoring, or persistence occurs
    And the job is completed with outcome FilteredNoise

  Scenario: Edge case — uncertain content is quarantined, not persisted
    Given an ExtractFact job whose computed trust score is 0.55
    And MEMORY_TRUST_QUARANTINE is 0.4 and MEMORY_TRUST_ADMIT is 0.7
    When the Write Pipeline processes the job
    Then no row is written to the fact table
    And one row is written to the quarantine table with trust_score 0.55
    And the job is completed with outcome Quarantined

  Scenario: Edge case — instruction-like content is capped below quarantine and rejected
    Given an ExtractFact job whose content matches the imperative-pattern detector
    When the Write Pipeline processes the job
    Then is_instruction_like is true
    And the trust score is capped strictly below MEMORY_TRUST_QUARANTINE
    And no row is written to the fact or quarantine tables
    And the job is completed with outcome Rejected

  Scenario: Edge case — high-confidence PII span is redacted in place
    Given an ExtractFact job whose content contains an email flagged with confidence 0.95
    And MEMORY_PII_REDACT_CONF is 0.9
    When the Write Pipeline processes the job
    Then the email substring is replaced by the literal token "‹redacted:‹email››"
    And the persisted fact carries pii_review = false

  Scenario: Edge case — low-confidence PII flag sets pii_review without redacting
    Given an ExtractFact job whose content contains a span flagged with confidence 0.6
    And MEMORY_PII_REDACT_CONF is 0.9
    When the Write Pipeline processes the job
    Then the span text is unchanged
    And the persisted fact carries pii_review = true

  Scenario: Error path — store unavailable triggers retryable failure
    Given an ExtractFact job that passes the write gate as Admit
    And MemoryStore::put_fact returns a StoreError indicating the store is unavailable
    When the Write Pipeline processes the job
    Then process returns AppError mapping to status 503 with code STORE_UNAVAILABLE
    And WorkQueue::fail is called with retryable = true

  Scenario: Error path — provider timeout retries then dead-letters at the attempt cap
    Given an ExtractFact job on its 5th attempt with MEMORY_JOB_MAX_ATTEMPTS = 5
    And EmbeddingClient::embed times out
    When the Write Pipeline processes the job
    Then process returns AppError mapping to status 504 with code PROVIDER_TIMEOUT
    And WorkQueue::fail is called with retryable = true
    And the queue moves the job to the dead_letter table

  Scenario: Error path — embedding dimension mismatch is terminal
    Given an ExtractFact job that passes the write gate as Admit
    And EmbeddingClient::embed returns a vector whose length != MEMORY_EMBED_DIM
    When the Write Pipeline processes the job
    Then process returns AppError mapping to status 400 with code VAL_OUT_OF_RANGE
    And WorkQueue::fail is called with retryable = false
```

#### Performance, Security, Observability

- **Performance targets.** Throughput-oriented and entirely **off the read path** (ADR-004); no read-path latency NFR (NFR-P2) applies. Per-job wall-clock budget: **`per_job_budget` = 30 s** (config), covering one LLM extract call, the PII scan, and one embed call plus store writes — each external call carries its own timeout (`tokio::time::timeout`) so the budget is bounded even when a provider hangs. Worker concurrency is a deployment knob (default: one consumer task; horizontally scalable because claim/lease is atomic in C2). Steady-state target: sustain the enqueue rate of the write class (SA-RATE-01 write limit, default 30 req/min/subject) per worker with headroom; backpressure is implicit — unclaimed jobs wait in the queue, they are never dropped. Memory ceiling per in-flight job: bounded by `MEMORY_MAX_BODY_BYTES` (1 MiB) content plus one embedding vector (`embed_dim * 4` bytes).
- **Security.** **Untrusted-source content is data, never instructions** (ADR-008): the imperative-pattern detector caps instruction-like content strictly below the quarantine floor so it can never be admitted; extracted content is never evaluated, executed, or used to steer pipeline control flow. The write gate (SA-WGATE-01) is the memory-poisoning defence. Scope is taken only from the job's `ScopeRef` (derived by C3 at enqueue), never from request-body content; every `MemoryStore` write is scoped to that tenant namespace. All store writes go through the `MemoryStore` trait whose SurrealDB implementation uses **parameterised** queries (bound `$values`), never string-concatenated content into SQL. PII handling per SA-PII-01: high-confidence spans redacted in place, lower-confidence flagged `pii_review`. No content, PII span text, secrets, or tokens are ever logged — only counts and ids. PII detection is in-process and deterministic (ADR-015): no PII endpoint or key exists. No hardcoded URLs/keys; the embed endpoint and key come from env (`MEMORY_EMBED_URL`, `MEMORY_EMBED_API_KEY`).
- **Observability.** Structured logs (each carrying `correlation_id`, `job_id`, `tenant`): `write.job.claimed`, `write.filtered_noise`, `write.intake` (`fact_count`), `write.entities.resolved` (`entity_count`, `created_count`), `write.pii.scanned` (`redacted_count`, `flagged_count`), `write.gate` (`decision`, `trust`, `is_instruction_like`), `write.persisted` (`fact_id`, `memory_class`, `confidence`, `salience`, `pii_review`), `write.job.failed` (`code`, `retryable`, `attempts`). Metrics (OpenTelemetry, labelled by `tenant` and `outcome`): counter `recall_write_jobs_total{outcome=persisted|quarantined|rejected|filtered_noise|failed}`; histogram `recall_write_job_duration_seconds`; histogram `recall_write_gate_trust_score`; counter `recall_write_pii_redactions_total`. Trace span `write.process` per job with child spans `write.intake`, `write.resolve_entities`, `write.pii_scan`, `write.gate`, `write.embed_and_persist`.

#### Gaps

None. *(Resolved in the Phase 3 reconciliation pass. Detail of how each was closed:)*

- **Entity-resolution surface — now canonical.** `MemoryStore::find_entity_by_name(ctx, name)`,
  `put_entity` (upsert), and `merge_entities(ctx, keep_id, merge_id)` are present in the Phase 2C.6
  trait surface and the C1 spec; step 4's lookup/upsert/merge uses them directly.
- **`pii_review` field — now on `Fact`.** Added to the §2C.2 `Fact` struct and the C1 `fact` table DDL
  (`DEFINE FIELD pii_review ON fact TYPE bool DEFAULT false`); step 8 sets it on the persisted fact.
- **`Source` upsert path — now canonical.** `MemoryStore::put_source` (upsert) and `get_source` are in
  the Phase 2C.6 surface and the C1 spec; `source_trust` is read from `Source.trust_signal`, defaulting
  to `MEMORY_SOURCE_TRUST_DEFAULT` for a newly-seen source.

> Note: `MEMORY_SOURCE_TRUST_DEFAULT` (f64, default 0.5, owner C4) is the prior-trust assigned to a
> newly-seen source; it is recorded here and carried into the Phase 4 Configuration spec / Phase 2D.

---

#### Amendment January 2026 — Collections (ADR-020)

> Source: HLD revision 0.11.0, ADR-020; requested by ordin's ADR-110. Counterparts: `service-layer.md` (puts `collection` in the job payload), `memory-store.md` (stores and validates it, migration `0007_fact_collection`), each amendment January 2026. Every rule below adds to the sections above.

##### Purpose (addition)

A write that names a collection lands in it: the fact, every entity and source the write creates, and a quarantined write's held record carry the request's `collection` (SA-COLL-01).

##### Approach (addition)

The label is carried, never judged: the gate's trust arithmetic, PII scan and instruction check are unchanged, so a write's collection cannot change whether it is admitted.

##### Shared Context (additions)

`JobPayload` gains `#[serde(default)] collection: Option<String>`; `QuarantineRecord` gains `#[serde(default)] collection: String`. `Fact`, `Entity` and `Source` carry `collection` as `memory-store.md` (amendment January 2026) defines.

##### Public Interface

N/A — no method changes.

##### Internal Logic (additions)

- **Step 0.** Deserialise `collection` with the payload; absent → `""`. A label breaking SA-COLL-01 is a malformed payload (the job is dead-lettered as step 0 already does for a malformed payload; C9 validated it, so this is a defence only).
- **Entity resolution.** A newly created entity takes the write's collection; an entity found by `find_entity_by_name` keeps its own.
- **Step 8.** The persisted fact and a newly created source take the write's collection.
- **Quarantine.** The held record keeps the collection.

##### Data Model (additive migration)

C4's portion of migration `0007_fact_collection` (the fact, entity, relationship and source portion is `memory-store.md`'s):

```sql
-- 0007_fact_collection.up.surql (C4 portion)
DEFINE FIELD IF NOT EXISTS collection ON quarantine TYPE string DEFAULT '';
-- 0007_fact_collection.down.surql (C4 portion)
REMOVE FIELD IF EXISTS collection ON quarantine;
```

`quarantine` is SCHEMAFULL, so without this field a held write carrying a collection would be refused outright (the case `0006` addressed for `fact_class`). Rollback: additive; the down path removes the field definition and the stored labels of held records.

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Job payload `collection` breaks SA-COLL-01 | job dead-lettered | — | — (logged `write.payload_invalid`) |
| Quarantine write without migration `0007` applied | 503 | STORE_UNAVAILABLE | — (job retried, then dead-lettered) |

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Write Pipeline — collections

  Scenario: Happy path — a write lands in its collection
    Given an ExtractFact job with content {"statement":"Payments owns the ledger"} and collection "tk:b" naming a new entity "Payments"
    When the job runs and the write gate admits it
    Then the fact and the entity "Payments" carry collection "tk:b"

  Scenario: Edge — an existing entity keeps its collection
    Given entity "Ledger" in the default collection
    When a "tk:b" write names "Ledger"
    Then the new fact carries "tk:b" and "Ledger" keeps ""

  Scenario: Edge — a quarantined write keeps its collection
    Given a "tk:b" write whose trust falls in the quarantine band
    When the job runs
    Then the quarantine record carries collection "tk:b"

  Scenario: Error — a malformed label in the payload
    Given a job payload with collection "Team B"
    When the job runs
    Then the job is dead-lettered and nothing is written
```

##### Performance, Security, Observability (additions)

- **Performance:** none; one string per row.
- **Security:** the label never affects the gate's decision.
- **Observability:** none added.

##### Gaps

None.

---

#### Amendment January 2026 — Entities, links and graph recall (ADR-021)

> Source: HLD revision 0.12.0, ADR-021; requested by ordin's ADR-111 (decision 91). Counterparts: `service-layer.md` (validates and enqueues the new fields), `memory-store.md` (`GraphStore`, closing rule, migration `0008_graph`), each amendment January 2026. Every rule below adds to the sections above and to the January 2026 collections amendment.

##### Purpose (addition)

A remember may now carry the entities the fact is about and the links the agent states between them. The pipeline resolves the entities with its existing three tiers, adds their aliases, checks names and kinds with the same personal-data scan as content and a narrower instruction check (known instruction phrases, step 7′), and — only when the write gate admits the fact — writes each link, or closes the live link an `ended` assertion names. A consolidated write carries the facts it summarises. No step calls a model (ADR-015): the agent states every entity and link.

##### Approach (addition)

Links ride on the fact's own job, so they share its gate decision, its retry and its idempotency: a replayed job reaches the same relationship ids and the same `relationship_fact` rows. Rejected: *a separate `WriteLink` job* — a link could land without its fact, or after the fact was quarantined; *storing names redacted* — a name with a redacted span would match nothing and still leak its shape, so such a name and its links are dropped.

Assumptions (Phase 1 1A): **SA-GRAPH-01** (entity assertions and the one normaliser), **SA-GRAPH-02** (link endpoints named in the request; admit path only; deterministic ids), **SA-GRAPH-03** (kind normalisation and merge).

##### Shared Context (additions)

`JobPayload` gains `#[serde(default)] entities: Vec<EntityAssertion>`, `#[serde(default)] links: Vec<LinkAssertion>`, `#[serde(default)] derived_from: Vec<String>` and `#[serde(default)] origin_teams: Vec<String>`, the teams C9 read `derived_from` under, set by C9 from the verified caller when `derived_from` is not empty and used for reading those origins alone; `QuarantineRecord` gains `#[serde(default)] entities_asserted: Vec<EntityAssertion>`, `links_asserted: Vec<LinkAssertion>` and `derived_from: Vec<String>`, because its `entities` is already the resolved-ids field, so a reviewer of a held write sees what it stated. `EntityAssertion` and `LinkAssertion` are the 2C.4 types (`phase-2-architecture.md`):

```rust
pub struct EntityAssertion { pub name: String /* 1..=512 */, #[serde(default)] pub aliases: Vec<String> /* 0..=32 */ }
pub struct LinkAssertion { pub from: String, pub kind: String /* 1..=128 */, pub to: String,
                           #[serde(default)] pub ended: bool, #[serde(default)] pub confidence: Option<f64> }
```

`GraphStore` (`find_live_relationship`, `relationship_kinds`, `link_fact`), `LinkRole` and `memory::text::normalise_name` are as `memory-store.md` (amendment January 2026) defines them. `Entity` also carries `private_aliases: Vec<String>`, those of `aliases` that came from the owner's private facts, and `alias_folders: Vec<AliasFolder>`, where `AliasFolder { alias: String /* normalised */, team: Option<String>, collection: String }`, never serialised to a caller.

##### Public Interface

`WritePipeline::resolve_entities` keeps its signature and its `Vec<String>` result. It wraps the crate-visible `resolve_mentions(ctx, ef, collection, visibility)`, which the pipeline calls and which returns `ResolvedEntities { ids: Vec<String>, by_name: HashMap<String, String>, created: Vec<String> }`: the fact's entity ids, the normalised name → entity id map steps 8′ and 4′ read, and the entities this resolution created, which an overtaken write removes again. `WriteOutcome` gains `Erased`. No other method changes.

##### Internal Logic (additions)

- **Step 0.** Deserialise `entities`, `links` and `derived_from` with the payload (absent → empty). Bounds and endpoint names were validated by C9; a breach here is a malformed payload and dead-letters the job (defence only).
- **Step 2 (addition).** Each `EntityAssertion` becomes an `EntityMention` with `surface = name`, after the subject/object mentions; a name that normalises to an existing mention's surface is merged with it.
- **Step 4′ (adds to step 4; runs last).** `by_name` is complete straight after step 4: it maps the normalised name and every normalised alias of each assertion, and the normalised subject and object, to the resolved entity id, so step 8′ can name an entity by an alias. The aliases themselves are stored as the job's last write, after step 8′ and the re-reads after persist, so a write rejected or overtaken after persist leaves no name on an entity; a failure here fails the job and the replay repeats the write. For each assertion with aliases whose resolved entity `get_entity` returns:
  1. An entity another user owns is left as it is: only an entity's owner adds to it, and the aliases resolve within this write alone (counted in `aliases_dropped`).
  2. Add the aliases: the union of the entity's existing aliases and the assertion's aliases, deduplicated by `normalise_name`, existing first, truncated at 32 (the number dropped logged as `aliases_dropped`).
  3. An alias a `UserPrivate` write adds also joins `Entity::private_aliases`, which the owner alone reads. A write of any other visibility stating an alias already held there removes it from `private_aliases`, which makes it public.
  4. Each stated alias that is stored records the folder that stated it, as `AliasFolder { alias: <normalised>, team: job.scope.team, collection }` in `Entity::alias_folders`, once per folder, when this write added the alias or the alias already has a folder row. An alias stored with no folder row keeps none, so no folder erasure removes it.
  5. Write with `put_entity` only when `aliases`, `private_aliases` or `alias_folders` changed.
- **Step 6′ (adds to step 6; runs after step 3, with step 7′, ahead of step 5).** Run `PiiDetector::scan` a second time over `{"names":[every name and alias, then every mention's surface], "kinds":[every link kind]}`. The mention surfaces include the names step 2 took from `content.subject` and `content.object`, which would otherwise become entity names while step 6 redacts them from the fact. A name with a span at `≥ pii_redact_conf` takes its whole assertion, the name and every alias, because its other names would still resolve to the entity it identified. Every mention holding one of those names and every link whose `from` or `to` is one of them go too (counted as `names_dropped`, `links_dropped`), so such a name reaches no entity; the fact keeps its redacted content, and if that was the only mention, step 4's anchor synthesis applies. A kind with such a span drops its link. A lower-confidence span, or a span whose pointer addresses neither list, sets `pii_review = true` on the fact. With no names and no links the scan does not run.
- **Step 7′ (adds to step 7).** The names-and-kinds object takes a narrower check than the fact's own text, because a name or a kind is a short noun phrase and a leading verb is ordinary in one ("run by", "Stop Light"). Each name, alias and kind is normalised with `normalise_kind` (lower-case, trim, collapse whitespace) and searched for these phrases of `INSTRUCTION_PHRASES`, each matched as a substring anywhere in the value: `ignore previous`, `ignore all previous`, `disregard previous`, `disregard the above`, `system prompt`, `you are now`, `new instructions`, `jailbreak`; and for `act as` as two whole words. `override`, which step 7 looks for, is not looked for here, because both occur in ordinary names ("Contract assurance", "Manual override"). The list of leading imperative verbs that step 7 applies does not apply here. One match rejects the write whole: nothing is written, no entity, no fact and no link, and the job completes as `Rejected` (logged `write.gate` with `decision = Reject`). The fact's own text keeps step 7 unchanged, with both the phrases and the leading verbs, so a fact whose `content.subject` starts with one of those verbs is still rejected at step 7; `Service::check_content` calls `detect_instruction` and keeps step 7's check too.
- **Step 8′ (adds to step 8, after `put_fact` and `set_fact_embedding`; admit path only).** For link i in request order:
  1. Resolve `from` and `to` through `by_name` after `normalise_name`; either missing (dropped in 6′) → skip, logged `write.link_skipped`.
  2. Normalise `kind` (lower-case, trim, collapse whitespace); then merge: exact match among `relationship_kinds(ctx, 1000)` → that kind; else the single existing kind with bigram Dice ≥ 0.92 → that kind; else the normalised kind.
  3. `ended = false`: `find_live_relationship(ctx, from, kind, to)`, which returns a live row of the write's team scope the caller can read, its own first; a row it cannot read behaves as absent, so two owners can hold live rows of one `from –kind→ to`. Found and owned by the caller → `put_relationship` with its `confidence` set to this assertion's (default 1.0), then `link_fact(rel, fact, Asserted)`; found and another user's → the row stays as it is, only `link_fact(rel, fact, Asserted)`, because the link's owner alone changes its row; none → `put_relationship` with id `relationship:<UUIDv5(NAMESPACE_DNS, "relationship:<fact id>:<i>")>`, `valid_from = now`, `valid_to = None`, `ingested_at = now`, `confidence`, `source_id` = the fact's `source_id`, owner = the fact's owner, `collection` = the write's collection, `replaces = None`; then `link_fact(.., Asserted)`.
  4. `ended = true`: `find_live_relationship`; found and owned by the caller → `end_relationship_validity(ctx, id, now)` then `link_fact(rel, fact, Ended)`; none, or another user's → nothing written, logged `write.link_end_unmatched`: a teammate's write cannot close a link.
  Any `StoreError` → `AppError::Store` (retryable); the replay reaches the same ids and upserts.
- **Step 8 (addition).** A consolidated write sets `Fact.derived_from` to the payload's `derived_from`, sorted and deduplicated. C9 checked readability and validity at intake; the pipeline re-checks each with `get_fact` under the job's scope with the caller's teams at intake (carried in the payload as `origin_teams`, because intake read the origins under all those teams and the job's scope holds one of them). The re-check runs after step 7 whatever the gate decided, so a queued summary of erased facts is neither stored nor held. If an origin is now absent or ended, the job completes as `Rejected` (logged `write.derived_from_stale`), and a replay also takes back what an earlier attempt stored (`take_back_fact`: `unlink_job_facts`, so the job survives the delete, then `hard_delete`, where `NotFound` is success). A summary is thus never kept over a fact that changed between intake and write.
- **After persist (admit path, after step 8′, before step 4′).** Two re-reads close the window embedding leaves open:
  1. `job_is_live`: an erasure deletes the jobs it covers before it reads what to erase, so either it saw this write or this read sees the job gone. Gone → `undo_write`: `hard_delete` the fact with its links, then `erase_entity` each entity of `ResolvedEntities.created`; a failure there is logged `write.undo_failed` and does not fail the job, the erasure having been reported already; a link the write closed is not reopened. Logged `write.erased_in_flight`; returns `WriteOutcome::Erased`. The rule holds for every erasure that removes jobs.
  2. The origins are read again as above, because an origin erased during the embedding leaves no job to see and no stored summary for its cascade to find. One absent or ended → `take_back_fact`, removing only the fact, since an origin can end without any erasure and `undo_write` would remove the writer's other pending work; logged `write.derived_from_stale`, returns `Rejected`.
- **Supersession order.** When the caller supersedes in the same flow (ordin's correction), the new fact's links are written by step 8′ before the caller's `supersede`, so `memory-store.md` step 12′ keeps a link the new fact repeats.
- **Quarantine.** No entity or link is written for a quarantined write; the held record keeps `entities_asserted`, `links_asserted` and `derived_from` for a reviewer. No operation releases a held record: the row stays until an erasure removes it, and nothing it holds reaches the `fact` table.

##### Data Model (additive migration)

C4's portion of migration `0008_graph` (the relationship, `relationship_fact`, entity and maintenance-state portion is `memory-store.md`'s):

```sql
-- 0008_graph.up.surql (C4 portion)
DEFINE FIELD IF NOT EXISTS entities_asserted ON quarantine TYPE any DEFAULT [];
DEFINE FIELD IF NOT EXISTS links_asserted    ON quarantine TYPE any DEFAULT [];
DEFINE FIELD IF NOT EXISTS derived_from      ON quarantine TYPE array<string> DEFAULT [];
-- 0008_graph.down.surql (C4 portion)
REMOVE FIELD IF EXISTS derived_from ON quarantine;
REMOVE FIELD IF EXISTS links_asserted ON quarantine;
REMOVE FIELD IF EXISTS entities_asserted ON quarantine;
```

`quarantine` is SCHEMAFULL, so the three fields must exist before a held write carries them (`QuarantineRecord`'s Rust fields take these column names; `entities` is already the resolved-ids column). The two assertion fields are `TYPE any`, not `array<object>`: SCHEMAFULL refuses the keys of an object nested in a typed array, so a held write stating one entity or link would fail on every attempt. Rollback: additive; the down path removes the held records' assertions.

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Payload `entities` / `links` / `derived_from` break their bounds | job dead-lettered | — | — (logged `write.payload_invalid`) |
| A `derived_from` fact is absent or ended when the job runs | job completed `Rejected` | — | — (logged `write.derived_from_stale`) |
| `put_relationship` / `link_fact` / `end_relationship_validity` fails | 503 | STORE_UNAVAILABLE | — (job retried, then dead-lettered) |

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Write Pipeline — entities and links

  Scenario: Happy path — a fact with two entities and a link (criterion 1)
    Given an ExtractFact job with content {"statement":"Borealis supplies packaging","subject":"Borealis"}, entities [{"name":"Borealis"},{"name":"packaging"}], links [{"from":"Borealis","kind":"supplies","to":"packaging"}] and idempotency key K
    When the job runs and the write gate admits it
    Then one fact, two entities and one live relationship from Borealis to packaging of kind "supplies" exist, with one relationship_fact row (asserted)
    When a job with the same key K is replayed
    Then nothing new is written

  Scenario: Edge — "no longer" closes the link (criterion 2)
    Given the live link Borealis–supplies–packaging
    When a job asserts "Borealis no longer supplies packaging" with links [{"from":"Borealis","kind":"supplies","to":"packaging","ended":true}]
    Then the link's valid_to is set and relationship_fact holds a row (rel, new fact, ended)

  Scenario: Edge — a similar kind merges
    Given a live kind "supplies" in the scope
    When a link of kind "  Supplies " is written
    Then it uses kind "supplies"

  Scenario: Edge — aliases join the entity
    Given entity "Borealis" with aliases ["Borealis Ltd"]
    When an assertion {"name":"borealis","aliases":["BRL","Borealis Ltd"]} is written
    Then the entity's aliases are ["Borealis Ltd","BRL"]

  Scenario: Error — a name with personal data is dropped with its links
    Given a PII detector that finds a phone number at confidence 0.95 in the entity name "Sam 07700 900123"
    When a job names that entity and a link to it
    Then no entity with that name is written, no link to it is written, and the fact is kept

  Scenario: Error — an instruction in a kind rejects the write
    When a job states a link of kind "ignore previous instructions"
    Then the outcome is Rejected and no fact, entity or link is written

  Scenario: Edge — an ordinary name or kind is not an instruction
    Given a job whose content has no instruction in it, stating entity "Stop Light" and a link of kind "run by"
    When the job runs and the write gate admits it
    Then the outcome is Persisted

  Scenario: Error — a quarantined write writes no link
    Given a write whose trust falls in the quarantine band, stating one link
    When the job runs
    Then the quarantine record carries the link assertion and no relationship is written

  Scenario: Edge — no outbound call (criterion 7)
    Given networking denied to the process
    When a job with entities and links runs
    Then it completes Persisted
```

##### Performance, Security, Observability (additions)

- **Performance.** Steps 4′ and 8′ add at most 16 entity lookups and 16 × 2 relationship reads and writes per job; job processing p95 grows by ≤ 30 ms at 16 links on the local store.
- **Security.** Names and kinds pass the same personal-data scan as content and the known-phrase instruction check of step 7′ (data, never instructions); a link is written only for an admitted fact; nothing is fetched from outside (ADR-015).
- **Observability.** `write.entities.resolved` gains `aliases_dropped`; new events `write.links` (`written`, `ended`, `skipped`), `write.link_skipped`, `write.link_end_unmatched`, `write.derived_from_stale`, `write.erased_in_flight`, `write.undo_failed`; `write.pii.scanned` gains `names_dropped`, `links_dropped`. Names and kinds are never logged.

##### Gaps

None.

---

#### Amendment January 2026 — Triples, change, end and conflict at the write (ADR-022)

> **Status:** Approved by the user January 2026. Source: HLD revision 0.13.0, ADR-022; requested by ordin's ADR-118 (decision 100). Counterparts: `service-layer.md` (validates the triple and the flags and enqueues them), `memory-store.md` (`TripleKey`, `live_same_subject_predicate`, `record_conflict`, migration `0011_triples`), each amendment January 2026 (ADR-022). Every rule below adds to the sections above and to the ADR-021 amendment.

##### Purpose (addition)

The pipeline now writes a fact whose content is one triple. It resolves the subject and an entity object with its existing tiers, adds the link an entity object asserts, and, for a write the caller states single-valued, either ends the earlier value (the caller said it **changed**), ends the same triple (the caller said it **ended**), or records a **conflict** with each disagreeing live fact. No step calls a model (ADR-015) and no step ends a fact on a judgement of its own.

##### Approach (addition)

The change, end and conflict run on the admit path after `put_fact`, on the fact's own job, so they share its gate decision, its retry and its idempotency, as links do (ADR-021). Rejected: *deciding change against conflict inside memory by comparing sources* — memory holds no vocabulary and cannot tell a newer version of a source from another source, so the caller states it (ADR-022 (2)); *recording conflicts in the maintenance cycle* — a person would see two disagreeing facts without the conflict until the next idle cycle.

Assumptions (Phase 1 1A): **SA-TRIPLE-04** (the link a triple asserts), **SA-TRIPLE-05** (the flags), **SA-TRIPLE-06** (change), **SA-TRIPLE-07** (end), **SA-TRIPLE-08** (conflict), **SA-TRIPLE-11** (passages), **SA-TRIPLE-14** (the writer's confidence and start).

##### Shared Context (additions)

`JobPayload` gains `#[serde(default)] single_valued: bool`, `#[serde(default)] changed: bool`, `#[serde(default)] ended: bool` `#[serde(default)] confidence: Option<f64>` and `#[serde(default)] valid_from: Option<DateTime<Utc>>` (SA-TRIPLE-14); `QuarantineRecord` keeps the content as sent, so a held write shows its triple. `Triple`, `TripleObject`, `TripleKey`, `ObjectKind` and `memory::text::normalise_predicate` are as `memory-store.md` (amendment ADR-022) defines them. `MEMORY_PASSAGE_CLASS` has one home, `Config.passage_class` (`src/config.rs`, default `"passage"`); `WritePipelineConfig` (`src/write_pipeline/mod.rs`) gains `passage_class: String`, copied from it by `WritePipelineConfig::from_config` as `trust_admit` is.

```rust
// src/write_pipeline/mod.rs — public (ordin change M-10 i): step 7a as a free function over any JSON.
// WritePipeline::detect_instruction calls it; its result is unchanged.
pub fn instruction_likelihood(content: &serde_json::Value) -> InstructionLikelihood;
```

##### Internal Logic (additions)

- **Step 0 (addition).** A payload whose `fact_class` is not the passage class and whose content does not parse as `Triple` is malformed (C9 validated it) and dead-letters the job (defence only), logged `write.payload_invalid`.
- **Step 2 (replaces the subject/object mentions of a triple).** For a triple: one mention with `surface = subject`; for an `entity` object, one mention with `surface = object.name`; a value object gives no mention. A passage keeps step 2 as before.
- **Step 2′ (addition; SA-TRIPLE-04).** For an `entity` object, add `LinkAssertion { from: subject, kind: predicate, to: object.name, ended: payload.ended, confidence: payload.confidence }` to the request's links, unless a link equal after `normalise_name` of the ends and exact comparison of the kind is already there; that link is then taken as the implied link and keeps its own `confidence`. Step 8′.2 does not lower-case or merge the implied link's kind, whether added here or taken: it is the predicate as stored (`normalise_predicate`), so the link's kind always equals the fact's predicate.
- **Step 5 (addition; SA-TRIPLE-14).** A payload with `confidence` set gives the fact that confidence as stated, and the write gate reads it; without it, the structured write scores with an extractor confidence of 1.0, as before (SA-SCORE-01).
- **Step 0′ (addition; SA-TRIPLE-14).** A payload whose `valid_from` is later than the job's `WorkJob.created_at` (C9 validated it at enqueue; defence only) completes `Rejected`, nothing written, logged `write.valid_from_future`.
- **Step 8 (addition; SA-TRIPLE-14).** `Fact.valid_from` = `payload.valid_from`, or `now` when absent; `ingested_at` stays `now`.
- **Step 6″ (adds to step 6′).** After the personal-data scan: a triple whose subject, or whose entity object, was dropped by step 6′, or whose `number`, `money` or `date` value the scan changed, states nothing checkable; the job completes `Rejected`, nothing is written, logged `write.triple_redacted`. A `text` value with a redacted span is kept redacted, as content always was, and sets `pii_review` as before.
- **Step 4″ (adds to step 4′).** After resolution, build the fact's `TripleKey`: `subject_entity` = `by_name[normalise_name(subject)]`; `predicate` = `normalise_predicate(predicate)`; for an entity object `object_entity` = `by_name[normalise_name(object.name)]` and `object_value` = that id; for a value, `object_value` and `object_unit` per SA-TRIPLE-02. Set `Fact.single_valued`, `Fact.changed` from the payload. `put_fact` stores them (store step 7″). A passage has `triple_key = None`.
- **Step 8″ (adds to step 8′; admit path only; runs after step 8′'s links and before the re-reads after persist).** For a triple with `single_valued` or `ended`:
  1. `live_same_subject_predicate(ctx, owner, subject_entity, predicate, 64)` — every visibility and collection of the owner — excluding the new fact itself.
  2. `ended` → the matching facts are those of the new fact's visibility and collection with the same `object_kind`, `object_value` and `object_unit`. For each, newest first, at most 16: `end_validity(ctx, id, t)`; the most recent is `supersede(ctx, id, new, t)` instead, where `t` = the later of the new fact's `valid_from` and the ended fact's own `valid_from` (SA-TRIPLE-14). Then `end_validity(ctx, new, fact.valid_from)` on the new fact itself (SA-TRIPLE-07). A matching fact of another user is not ended (SA-AUTHZ-07); none matched → only the new fact is ended. Logged `write.triple_ended { matched }`.
  3. `single_valued` and `changed` → the matching facts are those of the new fact's visibility and collection with a different object. The same ending and supersession as 2, without ending the new fact (SA-TRIPLE-06). Beyond 16, the rest, and every fact of another visibility or collection of the owner with a different object, are recorded as conflicts by 4's rule (a change is stated only for the writer's own visibility and collection, ADR-022 (3)). Logged `write.triple_changed { ended }`.
  4. `single_valued`, not `changed`, not `ended` → for each fact of any visibility and collection with a different object, at most 16: `record_conflict(ctx, new, id, now)` (SA-TRIPLE-08, ADR-022 (4)). Logged `write.conflicts { recorded }`.
  5. Not `single_valued`, not `ended` → nothing (two values of a multi-valued predicate never conflict).
  6. Author (SA-TRIPLE-15, added January 2026; Approved by the user January 2026). In 2 and 3, a write whose `RememberRequest.author` is set matches only facts whose `author` equals it; a write with no author matches facts of any author. A fact with the same object that 2 may not end stays live; a fact with a different object that 3 may not end is recorded as a conflict by 4's rule. When 2 leaves any fact with the same object live for this reason, logged `write.fact_not_ours { not_ours }`. Nothing is refused, so no error is returned.
  Any `StoreError` → `AppError::Store` (retryable); a replay ends nothing twice (`end_validity` of an ended fact is a no-op, `record_conflict` is an upsert).
- **Supersession order.** The new fact's own links (step 8′, including step 2′'s) are written before steps 8″.2 and 8″.3 supersede, so store step 12′ keeps a link the new fact repeats; a link of an ended triple closes because its only asserting fact ended.
- **After persist (adds to the re-reads).** `undo_write` also deletes the conflict rows the write recorded, by `hard_delete` of the fact (store 13⁗); facts the write ended are not reopened, as a closed link is not.
- **Passages (SA-TRIPLE-11).** A write whose `fact_class` is the passage class skips steps 2′, 4″, 6″ and 8″; its entities and links (ADR-021) are written as before.
- **Quarantine.** A held triple write ends nothing and records no conflict; the held record keeps the flags in its payload for a reviewer.

##### Data Model

N/A — the fields are `memory-store.md`'s (migration `0011_triples`).

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Payload content not a triple for a non-passage write | job dead-lettered | — | — (logged `write.payload_invalid`) |
| A payload `valid_from` later than the job's creation (defence; C9 refuses it first) | job completed `Rejected` | — | — (logged `write.valid_from_future`) |
| The subject or entity object dropped, or a number, money or date value changed, by the personal-data scan | job completed `Rejected` | — | — (logged `write.triple_redacted`) |
| `live_same_subject_predicate`, `end_validity`, `supersede` or `record_conflict` fails | 503 | STORE_UNAVAILABLE | — (job retried, then dead-lettered) |

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Write Pipeline — triples, change, end and conflict

  Scenario: Happy path — a triple with an entity object asserts its link
    Given an ExtractFact job with content {"subject":"Kestrel","predicate":"accountManager","object":{"kind":"entity","name":"Tom Reid"}} and single_valued true
    When the job runs and the write gate admits it
    Then one fact with subject_entity Kestrel, predicate "accountManager" and object_entity Tom Reid exists
    And one live relationship Kestrel –accountManager→ Tom Reid exists with one relationship_fact row (asserted)

  Scenario: Happy path — a value object asserts no link
    Given a job with content {"subject":"Order 7731","predicate":"price","object":{"kind":"money","amount":"12.50","currency":"GBP"}}
    When the job runs and the write gate admits it
    Then the fact has object_kind money, object_value "12.5" and object_unit "GBP", and no relationship is written

  Scenario: Edge — a stated change ends the earlier value
    Given live fact F1 "Kestrel accountManager Tom Reid" written single_valued
    When a job states "Kestrel accountManager Ana Silva" with single_valued true and changed true
    Then F1's valid_to is the new fact's valid_from, F1.superseded_by is the new fact, and the link to Tom Reid is closed
    And no fact_conflict row is written

  Scenario: Edge — a disagreement is recorded, not resolved
    Given live fact F1 "Kestrel accountManager Tom Reid" written single_valued
    When a job from another source states "Kestrel accountManager Ana Silva" with single_valued true and changed false
    Then both facts stay live and one fact_conflict row names both

  Scenario: Edge — a conflict reaches a fact of another visibility and collection of the same owner (ADR-022 (4))
    Given live fact F1 "Kestrel accountManager Tom Reid" of owner O, visibility tenant-shared, collection ""
    When a job of owner O, visibility team-shared, collection "team:folder-1" states "Kestrel accountManager Ana Silva" with single_valued true and changed false
    Then both facts stay live and one fact_conflict row names both

  Scenario: Edge — a stated change ends only facts of its own visibility and collection
    Given live fact F1 "Kestrel accountManager Tom Reid" of owner O, visibility tenant-shared
    When a job of owner O, visibility team-shared, states "Kestrel accountManager Ana Silva" with single_valued true and changed true
    Then F1 stays live and one fact_conflict row names both

  Scenario: Happy path — the fact and its link carry the writer's confidence (SA-TRIPLE-14)
    Given a job with an entity-object triple and confidence 0.4
    When the job runs and the write gate admits it
    Then the fact's confidence is 0.4 and the implied link's confidence is 0.4

  Scenario: Happy path — a stated start (SA-TRIPLE-14)
    Given live fact F1 "Kestrel accountManager Tom Reid" with valid_from 2025-01-01
    When a job states "Kestrel accountManager Ana Silva" with single_valued true, changed true and valid_from 2025-06-01
    Then the new fact's valid_from is 2025-06-01 and F1's valid_to is 2025-06-01

  Scenario: Error — a start later than the job (defence)
    Given a job whose payload valid_from is after its created_at
    When the job runs
    Then the outcome is Rejected and no fact is written

  Scenario: Edge — an explicit link equal to the implied one keeps the predicate as its kind
    Given a job whose triple predicate is "accountManager" and whose links hold {from "Kestrel", kind "accountManager", to "Tom Reid", confidence 0.7}
    When the job runs and the write gate admits it
    Then one relationship of kind "accountManager" with confidence 0.7 exists, and no relationship of kind "accountmanager"

  Scenario: Edge — two values of a multi-valued predicate do not conflict
    Given live fact "Borealis supplies packaging" written without single_valued
    When a job states "Borealis supplies cartons" without single_valued
    Then both facts stay live and no fact_conflict row is written

  Scenario: Edge — a triple that ended (SA-TRIPLE-07)
    Given live fact F1 "Borealis supplies packaging"
    When a job states the same triple with ended true
    Then F1 is ended and superseded by the new fact, the new fact is stored with valid_to equal to its valid_from, and the link is closed

  Scenario: Error — a redacted number is not stored
    Given a PII detector that finds a phone number at confidence 0.95 in the number value "07700900123"
    When the job runs
    Then the outcome is Rejected and no fact is written

  Scenario: Edge — a passage keeps its free content
    Given a job with fact_class "passage" and content {"statement":"Returns are accepted within 30 days.","subject":"Returns policy"}
    When the job runs and the write gate admits it
    Then the fact is stored with no triple fields and no conflict check runs

  Scenario: Edge — replay ends and records nothing twice
    Given the conflict job above completed
    When it is replayed with the same idempotency key
    Then still one fact_conflict row exists

  Scenario: Security — an agent ends only facts it wrote (SA-TRIPLE-15)
    Given agent A and agent B of one owner each kept the live fact "Borealis supplies packaging" with author "A" and author "B"
    When agent B writes the same triple with ended=true and author "B"
    Then B's fact is ended and A's fact stays live, with its link
    And "write.fact_not_ours" is logged with not_ours 1

  Scenario: Edge — a write with no author ends an agent's fact (SA-TRIPLE-15)
    Given agent A kept the live fact "Borealis supplies packaging" with author "A"
    When the owner writes the same triple with ended=true and no author
    Then A's fact is ended
```

##### Performance, Security, Observability (additions)

- **Performance.** Step 8″ adds one indexed lookup per single-valued or ended write and at most 16 ends or 16 conflict upserts; job processing p95 grows by ≤ 10 ms at 16 matches on the local store, measured by the C11 write leg.
- **Security.** The subject, an entity object and a text value pass the same personal-data scan and instruction checks as before (steps 6, 6′, 7, 7′); a value the scan alters is never stored as a different value (step 6″). Nothing is fetched from outside (ADR-015).
- **Observability.** New events `write.valid_from_future`, `write.triple_redacted`, `write.triple_ended { matched }`, `write.triple_changed { ended }`, `write.conflicts { recorded }`. No subject, predicate or value is logged.

##### Gaps

None.
