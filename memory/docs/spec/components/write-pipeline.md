### SPEC: Write Pipeline

**File:** `src/write_pipeline` | **Package:** `memory::write_pipeline` | **Phase:** 3 | **Dependencies:** C1 (Memory Store), C2 (Durable Work Queue)

> **Mode:** greenfield
> **derivedFromHld:** 0.12.0

#### Purpose

The Write Pipeline is the asynchronous consumer of `ExtractFact` work jobs from the durable work queue. It turns a structured, agent-asserted `remember` payload into a clean, scoped, provenance-tagged Fact in the Memory Store, or into a quarantined / rejected record when the content cannot be trusted. For each claimed job it runs eight ordered steps — noise filtering, structured-content intake, normalisation, entity resolution, scoring, PII scanning, the write gate, and embedding-then-persist. It never runs on the read path (ADR-004): a slow or failed write is retried with backoff behind the queue and is dead-lettered after the attempt cap, and a write never blocks a read. It is the component that enforces ADR-008 (write-gate trust validation and untrusted-data separation), SA-PII-01 (PII redaction / flagging), and the data-minimisation obligation (store structured assertions, drop low-signal noise).

#### Approach

The component is a **linear step pipeline driven by a claim/lease loop**, not a state machine and not a per-step actor graph. One async task per worker enumerates tenants (`MemoryStore::list_tenants`) and calls `WorkQueue::claim(tenant, &[JobKind::ExtractFact], lease)` across them round-robin (RISK-016 — the tenant is named, not inherited from connection state); each claimed job flows through the eight step functions in fixed order, and the first step that decides the job is finished (filtered as noise, rejected by the gate) short-circuits to `WorkQueue::complete`. This was chosen over a state-machine-per-job (rejected — the steps are a fixed acyclic sequence with no re-entry, so a state machine adds bookkeeping with no branching benefit) and over fanning each step to its own queue stage (rejected — every step shares the same job context and there is no independent scaling need across steps; multi-stage queuing would multiply queue round-trips against SA-LAT budgets for no gain). Entity resolution is a **three-tier escalation** (deterministic rules → ML similarity → create-new) so the cheap tiers settle the common cases and a genuinely ambiguous identity yields a fresh entity rather than risking a wrong merge; recall holds no LLM (ADR-015), so create-new is the terminal tier by design (not a deferral). The component owns only the `quarantine` table; the `fact` and `entity` tables are owned by C1 and written through the `MemoryStore` trait.

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
pub enum WriteOutcome { Persisted, Quarantined, Rejected, FilteredNoise }

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

`process(ctx, job)` runs the steps below in order. Each step states what it does, which dependency it calls, which errors it can raise, and what it logs. All logs are structured and carry `correlation_id` (from `job.scope`/`ctx`), `job_id`, and `tenant`; content values and PII are never logged.

0. **Validate job payload.** Deserialise `job.payload` into `{ content: Value, source: Option<SourceInput>, memory_class: Option<MemoryClass>, fact_class: Option<String> }`. `fact_class` is the caller-owned class C9 **already authorised** against the token's `memory_fact_authority` claim (SA-AUTHZ-01); C4 carries it to storage and judges nothing about it — it validates no vocabulary, compares it against nothing, and treats no value specially (SA-AUTHZ-09, SA-LLMFREE-01). A job that carries none is an unclassed write and stays unclassed. If `job.kind != ExtractFact`, raise `AppError::Validation` (terminal — wrong consumer claimed it; this is impossible under the claim filter but defended). If `content` is absent or not a JSON object, raise `AppError::Validation` mapped to `VAL_INVALID_BODY` (terminal — the payload must carry a structured assertion object). Build the internal `ScopeContext` from `job.scope`. Logs: `write.job.claimed` with `job_id`, `tenant`, `attempts`.

1. **Filter noise** (`filter_noise`). Drop the job as noise when, after trimming, the serialised `content` is empty, is an empty object/array, or its total non-whitespace string length across all string values is `< 3` characters. On noise: log `write.filtered_noise`, call `WorkQueue::complete(job.id)`, return `WriteOutcome::FilteredNoise`. Errors: none (pure). This is the data-minimisation step (HLD 06).

2. **Intake** (`intake`). Wrap the structured agent-asserted `content` as a single `ExtractedFact` with `entities` derived from the content's referenced entities (a deterministic `scan_entity_mentions` scan over the well-known `subject`/`object` string fields), `memory_class` taken from the request's `memory_class` (else `Episodic`), and `confidence = 1.0` (the caller asserts it directly). **No LLM call** (ADR-015 — recall holds no LLM); there is no provider-error path here. If the wrapped content yields no salient assertion (empty after intake), complete the job as `FilteredNoise`. Logs: `write.intake` with `fact_count`. **Security:** the content is treated as data, never executed or interpreted as instructions to the pipeline.

   Steps 3–8 then run **once per `ExtractedFact`** in the returned vector; each produces its own gate decision and persistence outcome. A failure on one fact aborts the whole job as retryable (the queue replays all facts; persistence is idempotent on the derived fact id — see step 8).

3. **Normalise** (`normalise`). Canonicalise the `ExtractedFact.content`: sort object keys (recursively, lexicographic), collapse internal whitespace runs to single spaces and trim string values, and apply Unicode NFC normalisation; each `EntityMention.surface` is whitespace-collapsed the same way. Pure; no dependency. Errors: none. Logs: none (per-fact, high volume). **Assumption (NFC):** the as-built `nfc` helper is the identity function — input is expected to arrive already NFC-composed, so the canonicalisation that affects recall is key order plus whitespace; full Unicode decomposition / recomposition is deferred to avoid a heavy dependency. This is a documented assumption, not a gap.

4. **Entity-resolve** (`resolve_entities`) — three-tier escalation, returning `>=1` entity ids. For each `EntityMention`: (a) **Rules tier** — normalise the `surface` (NFC, lowercase, trim, collapse whitespace, strip leading/trailing ASCII punctuation; a normalised-empty mention is skipped) and look up an existing `Entity` by exact canonical-name or alias match within `ctx` via `MemoryStore::find_entity_by_name`. On a unique hit, resolve to it; on **multiple** exact hits, deterministically pick the lexicographically-smallest entity id rather than guess (a later maintenance merge folds the duplicates, C7). (b) **ML tier** — on no exact hit, run a second `find_entity_by_name` against the *raw* surface form (a case / punctuation variant) and score each candidate's canonical name against the normalised surface with a character-bigram Sørensen–Dice coefficient; a single candidate at or above the similarity admit threshold (`ML_SIMILARITY_ADMIT = 0.92`) resolves, otherwise escalate. (c) **create-new tier (v1)** — on ambiguity, the mention is resolved by creating a **new** `Entity` via `MemoryStore::put_entity` (canonical_name = normalised surface form, the original surface form added to `aliases`) — never merging into an existing entity without an above-threshold match, and **never silently overwriting** an existing entity (HLD 04). The HLD's third tier named LLM adjudication, but memory has no LLM (ADR-015), so create-new is the terminal tier by design (not a deferral). Duplicate entities created this way are folded later by the C7 Maintenance Worker via `MemoryStore::merge_entities`. If, after processing every mention, no entity id was produced (the extractor offered no usable mention), the pipeline synthesises a single anchor entity from the content (a normalised `subject` string, else the first non-empty string leaf) so a salient assertion is still anchored rather than dropped; only when that synthesis also yields an empty name does it raise `AppError::Validation` (terminal — a fact must connect `>=1` entity, 2C.2). Errors: `StoreError` → `AppError::Store` (retryable). Logs: `write.entities.resolved` with `entity_count`, `created_count`.

5. **Score** (`score`). Compute `salience` and `confidence`, each clamped to `[0,1]` (SA-SCORE-01). `confidence = clamp01(ef.confidence * source_trust_factor)` where `source_trust_factor = 0.5 + 0.5 * source_trust.clamp(0,1)` (a source-less agent assertion → `source_trust = 1.0`). `salience` is a bounded heuristic in `[0,1]` from content richness: base `0.3`, `+0.15` per entity mention (mention weight capped at `0.45`), `+0.2` when the content names a `predicate` / `relation` / `verb` field. Pure. Errors: none. Logs: none.

6. **PII scan** (`pii_scan`). Call `PiiDetector::scan(&content)` → `Vec<PiiSpan>`. For each span with `confidence >= cfg.pii_redact_conf`: locate the string via `json_pointer`, replace bytes `[start,end)` with the literal `‹redacted:‹{pii_type}››` (SA-PII-01). If any span has `confidence < cfg.pii_redact_conf`, set `pii_review = true`. Return `(redacted_content, pii_review)`. Errors: `ProviderError` → `AppError::Provider` (retryable iff timeout/5xx). Logs: `write.pii.scanned` with `redacted_count`, `flagged_count` — never the span text or type values beyond counts.

7. **Write gate** (`detect_instruction` then `write_gate`) — ADR-008 / SA-WGATE-01. (a) `detect_instruction` runs the **imperative-pattern detector** over the content's string values: it matches imperative-mood leading verbs and known prompt-injection markers (case-insensitive). Contract: it returns `InstructionLikelihood { is_instruction_like, matched_patterns }`; `is_instruction_like` is `true` when `matched_patterns >= 1`. (b) `write_gate` computes `trust = clamp01(0.6 * confidence + 0.4 * source_trust)`; if `instr.is_instruction_like`, `trust = min(trust, cfg.trust_quarantine - 0.0001)` (capped strictly below the quarantine floor so instruction-like content can never admit or quarantine). Then: `trust >= cfg.trust_admit` → `Admit`; `cfg.trust_quarantine <= trust < cfg.trust_admit` → `Quarantine`; `trust < cfg.trust_quarantine` → `Reject`. The three outcomes are distinguishable and each is audited (log `write.gate` with `decision`, `trust`, `is_instruction_like`). On `Reject`: persist nothing, `WorkQueue::complete`, return `WriteOutcome::Rejected`. On `Quarantine`: call `quarantine(ctx, qr)` to write a `QuarantineRecord` — carrying `fact_class` alongside the content and scores, so a reviewer sees what the write *claimed to be* and not only what it contained (SA-AUTHZ-09) — to the `quarantine` table, `WorkQueue::complete`, return `WriteOutcome::Quarantined`. Errors (quarantine path): `StoreError` → `AppError::Store` (retryable).

8. **Embed and persist** (`embed_and_persist`) — admit path only. Render the (possibly redacted) `content` to a single keyword string via `content_to_text` (mirroring the C1 store so the embedded text matches the indexed text) and call `EmbeddingClient::embed(&[text])`; take the first returned vector (a missing vector raises terminal `VAL_OUT_OF_RANGE`) and assert its length `== cfg.embed_dim` (SA-EMBED-01) else raise `AppError::Validation` (terminal — a dimension mismatch is a config/provider defect, not retryable). Construct the `Fact`: `id` = `fact:<uuid>` derived deterministically (UUIDv5 over the seed `"fact:<idempotency_key>:<fact_index>"`, DNS namespace) when `idempotency_key` is present so a queue replay reuses the same id (idempotent persist), else a fresh UUIDv7; `entities` from step 4; `source_id` from a `Source` upsert when `source` was provided, else `None`; `memory_class` from the `ExtractedFact`; `fact_class` from the payload verbatim (SA-AUTHZ-09) — and on a **superseding** write it is the class of *this* write, never inherited from the superseded fact, because a supersession is a new assertion that passed the gate with its own token (SA-AUTHZ-10); `visibility = UserPrivate` (SA-VIS-01 default); `owner = job.scope`; `valid_from = now`; `valid_to = None`; `ingested_at = now`; scores from step 5; `stability = 1.0`; `pii_review` from step 6, carried on the stored fact (the §2C.2 `Fact` and the C1 `fact` table both carry `pii_review`); `supersedes/superseded_by/derived_from` empty; `last_recalled_at = None`. Call `MemoryStore::put_fact(&fact)` then `MemoryStore::set_fact_embedding(ctx, &fact.id, &vector, embed_model_version)` to attach the embedding and the `MEMORY_EMBED_MODEL_VERSION` it was produced with (re-embed bookkeeping for C7). Errors: `StoreError` → `AppError::Store` (retryable); `ProviderError` (embed) → `AppError::Provider` (retryable iff timeout/5xx). On success: `WorkQueue::complete`, return `WriteOutcome::Persisted`. Logs: `write.persisted` with `fact_id`, `memory_class`, `confidence`, `salience`, `pii_review`.

**Failure / retry handling (SA-QUEUE-02).** Any step that returns a **retryable** `AppError` (`Store`, `Queue`, timeout/5xx `Provider`) causes `process` to propagate it; the consumer loop then calls `WorkQueue::fail(job.id, retryable = true)`. C2 owns the backoff schedule (base 2 s, factor 2, jitter ±20 %) and moves the job to `dead_letter` once `attempts >= cfg.max_attempts`. Any **terminal** `AppError` (`Validation`, non-retryable `Provider`, dimension mismatch) causes `WorkQueue::fail(job.id, retryable = false)`, which C2 dead-letters immediately. The pipeline itself never sleeps for backoff and never deletes a job; both are queue responsibilities. A write being slow or failing here never affects a read (ADR-004) — there is no shared lock between this loop and the read path.

#### Data Model

This component owns one table: `quarantine`. The `fact` and `entity` tables are owned by **C1 (Memory Store)** and are written only through the `MemoryStore` trait — this spec does **not** redefine their DDL. Fields this component sets on a persisted `Fact` are listed in *Internal Logic* step 8.

The `quarantine` DDL ships inside the **single squashed initial migration** `migrations/0001_init.{up,down}.surql` (version 1, slug `init`), not a separate `0003_quarantine` migration: the five original numbered migrations were folded into one initial migration before first release (greenfield, no shipped database — FU-018). The Migrator selects the tenant namespace (`USE NS <tenant>; USE DB memory`) before running the file, so the table is per-tenant and isolation is structural (ADR-011). Every `DEFINE` uses `IF NOT EXISTS` so the migration is idempotent.

The store is schemaless for the C1 domain tables (SA-MIG-01); `quarantine` is declared **schemafull** because its shape is stable and audit-critical. Per the SurrealDB 3.x adaptation baked into the migration, the arbitrary nested-JSON `content` field is `TYPE object FLEXIBLE` (3.x requires `FLEXIBLE` after `TYPE object` and rejects un-enumerated nested keys on a bare `TYPE object`). Domain ids are stored as `"table:key"` strings throughout (see `src/store/convert.rs`), so the link-shaped fields `entities` and `source_id` are `array<string>` / `option<string>`, not native record links — matching how the C1 `fact` table stores the same ids.

```sql
-- C4 — Write Pipeline: quarantine (gate-uncertain content awaiting review).
-- Holds gate-uncertain content (trust in [trust_quarantine, trust_admit)) for review.
-- One row per quarantined ExtractedFact; never an admitted or rejected fact.
DEFINE TABLE IF NOT EXISTS quarantine SCHEMAFULL;

DEFINE FIELD IF NOT EXISTS content            ON quarantine TYPE object FLEXIBLE; -- redacted content (PII removed)
DEFINE FIELD IF NOT EXISTS entities           ON quarantine TYPE array<string>;   -- resolved entity ids (>=1)
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
DEFINE FIELD IF NOT EXISTS idempotency_key    ON quarantine TYPE option<string>;  -- carried from the originating job
DEFINE FIELD IF NOT EXISTS reason             ON quarantine TYPE string;          -- audit: why quarantined
DEFINE FIELD IF NOT EXISTS quarantined_at     ON quarantine TYPE datetime;        -- server-set, RFC3339 ms

DEFINE INDEX IF NOT EXISTS quarantine_owner_idx ON quarantine FIELDS owner.user, quarantined_at;
DEFINE INDEX IF NOT EXISTS quarantine_idem_idx  ON quarantine FIELDS idempotency_key UNIQUE;  -- replay-safe
```

The Rust mirror:

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct QuarantineRecord {
    pub id: String,                       // "quarantine:<uuidv7>"
    pub content: serde_json::Value,       // redacted content (object)
    pub entities: Vec<String>,            // entity ids (>=1)
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
    pub idempotency_key: Option<String>,
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

Non-error terminal outcomes (not failures, no error code): **FilteredNoise** and **Rejected** → `WorkQueue::complete`; **Quarantined** → write `QuarantineRecord` then `WorkQueue::complete`. These are audited via the `write.gate` / `write.filtered_noise` logs, not via the error registry.

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
- **Quarantine.** The held record keeps the collection, and a released record is persisted into it.

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

A remember may now carry the entities the fact is about and the links the agent states between them. The pipeline resolves the entities with its existing three tiers, adds their aliases, checks names and kinds with the same personal-data scan and instruction check as content, and — only when the write gate admits the fact — writes each link, or closes the live link an `ended` assertion names. A consolidated write carries the facts it summarises. No step calls a model (ADR-015): the agent states every entity and link.

##### Approach (addition)

Links ride on the fact's own job, so they share its gate decision, its retry and its idempotency: a replayed job reaches the same relationship ids and the same `relationship_fact` rows. Rejected: *a separate `WriteLink` job* — a link could land without its fact, or after the fact was quarantined; *storing names redacted* — a name with a redacted span would match nothing and still leak its shape, so such a name and its links are dropped.

Assumptions (Phase 1 1A): **SA-GRAPH-01** (entity assertions and the one normaliser), **SA-GRAPH-02** (link endpoints named in the request; admit path only; deterministic ids), **SA-GRAPH-03** (kind normalisation and merge).

##### Shared Context (additions)

`JobPayload` gains `#[serde(default)] entities: Vec<EntityAssertion>`, `#[serde(default)] links: Vec<LinkAssertion>` and `#[serde(default)] derived_from: Vec<String>`; `QuarantineRecord` gains the same three fields with `#[serde(default)]`, so a released record writes its links. `EntityAssertion` and `LinkAssertion` are the 2C.4 types (`phase-2-architecture.md`):

```rust
pub struct EntityAssertion { pub name: String /* 1..=512 */, #[serde(default)] pub aliases: Vec<String> /* 0..=32 */ }
pub struct LinkAssertion { pub from: String, pub kind: String /* 1..=128 */, pub to: String,
                           #[serde(default)] pub ended: bool, #[serde(default)] pub confidence: Option<f64> }
```

`GraphStore` (`find_live_relationship`, `relationship_kinds`, `link_fact`), `LinkRole` and `memory::text::normalise_name` are as `memory-store.md` (amendment January 2026) defines them.

##### Public Interface

`WritePipeline::resolve_entities` keeps its signature; it now also returns, per assertion index, the resolved entity id (`ResolvedEntities { ids: Vec<String>, by_name: HashMap<String, String> }` — the normalised name or alias → entity id map step 8′ reads). No other method changes.

##### Internal Logic (additions)

- **Step 0.** Deserialise `entities`, `links` and `derived_from` with the payload (absent → empty). Bounds and endpoint names were validated by C9; a breach here is a malformed payload and dead-letters the job (defence only).
- **Step 2 (addition).** Each `EntityAssertion` becomes an `EntityMention` with `surface = name`, after the subject/object mentions; a name that normalises to an existing mention's surface is merged with it.
- **Step 4′ (adds to step 4).** For each assertion's resolved entity, add its aliases: the union of the entity's existing aliases and the assertion's aliases, deduplicated by `normalise_name`, existing first, truncated at 32 (the number dropped logged as `aliases_dropped`); write with `put_entity` only when the set changed. `by_name` maps the normalised name and every normalised alias of each assertion, and the normalised subject and object, to the resolved entity id.
- **Step 6′ (adds to step 6).** Run `PiiDetector::scan` a second time over `{"names":[every name and alias], "kinds":[every link kind]}`. A name or alias with a span at `≥ pii_redact_conf` is removed before step 4′ writes, together with every link whose `from` or `to` is that name (counted as `names_dropped`, `links_dropped`); if it was the only mention, step 4's anchor synthesis applies. A kind with such a span drops its link. A lower-confidence span sets `pii_review = true` on the fact.
- **Step 7′ (adds to step 7).** `detect_instruction` also runs over the names-and-kinds object; a match makes the write instruction-like, so its trust is capped below the quarantine floor and the write is rejected whole.
- **Step 8′ (adds to step 8, after `put_fact` and `set_fact_embedding`; admit path only).** For link i in request order:
  1. Resolve `from` and `to` through `by_name` after `normalise_name`; either missing (dropped in 6′) → skip, logged `write.link_skipped`.
  2. Normalise `kind` (lower-case, trim, collapse whitespace); then merge: exact match among `relationship_kinds(ctx, 1000)` → that kind; else the single existing kind with bigram Dice ≥ 0.92 → that kind; else the normalised kind.
  3. `ended = false`: `find_live_relationship(ctx, from, kind, to)`; found → `put_relationship` with its `confidence` set to this assertion's (default 1.0), then `link_fact(rel, fact, Asserted)`; none → `put_relationship` with id `relationship:<UUIDv5(NAMESPACE_DNS, "relationship:<fact id>:<i>")>`, `valid_from = now`, `valid_to = None`, `ingested_at = now`, `confidence`, `source_id` = the fact's `source_id`, owner = the fact's owner, `collection` = the write's collection, `replaces = None`; then `link_fact(.., Asserted)`.
  4. `ended = true`: `find_live_relationship`; found → `end_relationship_validity(ctx, id, now)` then `link_fact(rel, fact, Ended)`; none → nothing written, logged `write.link_end_unmatched`.
  Any `StoreError` → `AppError::Store` (retryable); the replay reaches the same ids and upserts.
- **Step 8 (addition).** A consolidated write sets `Fact.derived_from` to the payload's `derived_from`, sorted and deduplicated. C9 checked readability and validity at intake; the pipeline re-checks each with `get_fact` under the job's scope and, if one is now absent or ended, completes the job as `Rejected` (logged `write.derived_from_stale`), so a summary is never kept over a fact that changed between intake and write.
- **Supersession order.** When the caller supersedes in the same flow (ordin's correction), the new fact's links are written by step 8′ before the caller's `supersede`, so `memory-store.md` step 12′ keeps a link the new fact repeats.
- **Quarantine.** No link is written for a quarantined write; the held record keeps `entities`, `links` and `derived_from`, and a release runs steps 4′ and 8′.

##### Data Model (additive migration)

C4's portion of migration `0008_graph` (the relationship, `relationship_fact`, entity and maintenance-state portion is `memory-store.md`'s):

```sql
-- 0008_graph.up.surql (C4 portion)
DEFINE FIELD IF NOT EXISTS entities_asserted ON quarantine TYPE array<object> DEFAULT [];
DEFINE FIELD IF NOT EXISTS links_asserted    ON quarantine TYPE array<object> DEFAULT [];
DEFINE FIELD IF NOT EXISTS derived_from      ON quarantine TYPE array<string> DEFAULT [];
-- 0008_graph.down.surql (C4 portion)
REMOVE FIELD IF EXISTS derived_from ON quarantine;
REMOVE FIELD IF EXISTS links_asserted ON quarantine;
REMOVE FIELD IF EXISTS entities_asserted ON quarantine;
```

`quarantine` is SCHEMAFULL, so the three fields must exist before a held write carries them (`QuarantineRecord`'s Rust fields `entities`, `links`, `derived_from` map to these column names; `entities` is already the resolved-ids column). Rollback: additive; the down path removes the held records' assertions, so a record released afterwards is kept without its links.

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
- **Security.** Names and kinds pass the same personal-data scan and instruction check as content (data, never instructions); a link is written only for an admitted fact; nothing is fetched from outside (ADR-015).
- **Observability.** `write.entities.resolved` gains `aliases_dropped`; new events `write.links` (`written`, `ended`, `skipped`), `write.link_skipped`, `write.link_end_unmatched`, `write.derived_from_stale`, `write.pii.scanned` gains `names_dropped`, `links_dropped`. Names and kinds are never logged.

##### Gaps

None.
