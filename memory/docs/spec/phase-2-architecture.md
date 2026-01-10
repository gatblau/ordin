# Phase 2 — Architecture Artefacts

> **Spec set:** `memory` (agentic memory service) · **Mode:** greenfield
> **derivedFromHld:** 0.12.0 · **Source HLD:** `docs/design/agentic-memory/` · **Authored:** January 2026 · **Amended:** January 2026 (RFC 01, ADR-014; RFC 02, ADR-015), January 2026 (RFC 01-MCP, ADR-016), January 2026 (ordin ADR-110, ADR-020), January 2026 (ordin ADR-111, ADR-021 — entities, links and graph recall)

## Table of contents

- [2A — System Context Diagram](#2a--system-context-diagram)
- [2B — Component Inventory](#2b--component-inventory)
- [2C — Shared Types Catalogue](#2c--shared-types-catalogue)
- [2D — Configuration & Environment Variables](#2d--configuration--environment-variables)

Stack is committed by the HLD and is binding for every component spec: **Rust** (ADR-009), `tokio`
async runtime + `axum` HTTP, **embedded SurrealDB** (ADR-003/009), store-backed durable work queue
(SA-QUEUE-01), `openidconnect`/`jsonwebtoken` for OIDC (ADR-001), `opentelemetry` for
logs/metrics/traces. One crate, `recall`, with one module per component under `src/`.

---

## 2A — System Context Diagram

Every arrow is labelled with protocol + authentication. Internal in-process calls are Rust function
calls (no network, no auth hop) and are labelled `in-proc`.

```mermaid
flowchart TB
    broker["Broker (as user)"]
    idp[(OIDC Identity Provider)]
    embed["Embedding provider"]
    rerank["Reranker (cross-encoder)"]
    obs["Observability stack"]

    subgraph recall["recall (single Rust binary)"]
        api["HTTP API Edge<br/>src/api"]
        auth["Auth & Scope<br/>src/auth"]
        retr["Retrieval Engine<br/>src/retrieval"]
        queue["Durable Work Queue<br/>src/queue"]
        wpipe["Write Pipeline<br/>src/write_pipeline"]
        maint["Maintenance Worker<br/>src/maintenance"]
        store[("Memory Store<br/>src/store — embedded SurrealDB")]
    end

    broker -->|HTTPS + Authorization: Bearer OIDC JWT| api
    api -->|in-proc| auth
    auth -->|HTTPS GET .well-known + JWKS, no auth| idp
    api -->|in-proc, post-authz| retr
    api -->|in-proc, enqueue job| queue
    api -->|in-proc, read fact by id| store
    retr -->|in-proc| store
    retr -->|HTTPS + provider API key, query embed| embed
    retr -->|HTTPS + provider API key, rerank top-k| rerank
    queue -->|in-proc, claim job| wpipe
    queue -->|in-proc, claim job| maint
    wpipe -->|in-proc, persist| store
    wpipe -->|HTTPS + provider API key, fact embed| embed
    maint -->|in-proc| store
    maint -->|HTTPS + provider API key, re-embed| embed
    recall -->|OTLP/gRPC + bearer, logs/metrics/traces| obs
```

**Trust boundaries (from HLD 01):** the broker is the only authenticated API caller; identity is
trusted from the OIDC token's issuer, never from the request body; source content is untrusted data,
gated on the write path; `memory` holds no end-user credentials.

---

## 2B — Component Inventory

`Type`: `service-component` (owns a flow) · `infrastructure` (cross-component substrate) ·
`store` (persistence). `Phase` is the build order (a valid DAG — each component depends only on
lower-numbered phases). `Complexity`: S/M/L/XL relative effort.

| # | Component | Type | Phase | Dependencies (internal) | Complexity |
|---|---|---|---|---|---|
| C1 | **Memory Store** (`src/store`) | store | 1 | none | XL |
| C2 | **Durable Work Queue** (`src/queue`) | infrastructure | 2 | C1 | M |
| C3 | **Auth & Scope** (`src/auth`) | service-component | 2 | none (shared types only) | L |
| C4 | **Write Pipeline** (`src/write_pipeline`) | service-component | 3 | C1, C2 | XL |
| ~~C5~~ | ~~**Freshness Checker**~~ | — | — | — | **Retired by ADR-014** — freshness is agent-side; `memory` performs no source-change check. The id `C5` is retired (not reused) to keep C6/C7/C8 stable. |
| C6 | **Retrieval Engine** (`src/retrieval`) | service-component | 4 | C1, C7 (pure decay functions `reinforce` and `is_fading` only; edge recorded January 2026, ADR-021) | XL |
| C7 | **Maintenance Worker** (`src/maintenance`) | service-component | 4 | C1, C2 | XL |
| C9 | **Service Layer** (`src/service`) | service-component | 5 | C1, C2, C3, C6 | L |
| C8 | **HTTP API Edge** (`src/api`) | service-component | 6 | C9 | M |
| C10 | **MCP API Edge** (`src/mcp`, bin `memory-mcp`) | service-component | 6 | C9 | M |
| C11 | **Tenancy Benchmark Harness** (`src/bench`, bin `memory-bench`) | tooling | 7 | C1 | L |
| C12 | **Consent Grants** (`src/types/grant.rs`, `migrations/0004_consent_grants.up.surql`, the read arm in `src/store`; `src/grants` not yet created) | service-component | 2 | C1, C3 (shared types only) | M |

> **Change (ADR-016, RFC 01).** C9 is **new**: the transport-agnostic orchestration extracted from the
> former C8 (`EdgePipeline`) — auth → authorise → rate-limit → idempotency → component → audit → X1
> error-classification, keyed on a verified `ScopeContext` and the 2C DTOs (SA-SVC-01). C8 is
> **refactored** from "owns orchestration" to a thin HTTP adapter over C9 (its external contract is
> byte-for-byte preserved). C10 is **new**: a second thin adapter exposing the same operations as MCP
> tools over streamable-HTTP, in its own binary `memory-mcp` (SA-BIN-01, SA-MCP-TRANSPORT-01). Both
> edges build from the same `build_state`.

> **Change (January 2026, `sync-check` F-005 → `sync-spec`).** **C12 (Consent Grants)** is added to
> this inventory. It is not new work — it has had a full Phase 3 spec since the Companion Platform's
> OQ-C1, and its read half has shipped — but it carried no component number and no row here, so every
> tool that enumerates components from this table missed it, including `sync-check` itself. C5 stays
> retired and unreused, so C12 is the next free id. **Take care reading it in prose:** `C12` also
> names a `codegen` authoring rule (minimise public surface), cited in `phase-1-analysis.md` and
> `components/memory-store.md`; the component is written `C12 (Consent Grants)` and the rule `rule
> C12` wherever both could be meant.
>
> C12 is **half-built and its halves are not co-located**, which is why its `**File:**` field names
> three paths. Built: the types (`src/types/grant.rs`), the `grant` table and its two indexes
> (`migrations/0004_consent_grants.up.surql`), and the fourth arm of `fact_filter_clause`
> (`src/store/mod.rs`). Not built: `issue`, `revoke`, `list` and any transport exposure — there is no
> `src/grants` module, and no edge or service surface references a grant. Its Phase 2 build order and
> its dependency on C3 are shared-types only (`ScopeContext`), so the DAG rule above is not weakened:
> nothing in C12 waits on C3 behaviour.

> **Change (RFC 02-consumer-tenancy-profile, pass 1 — benchmark; legs re-pointed by RFC 03).** C11 is
> **new** and carries a new `Type` value, `tooling`: a component that is built from the crate and
> shipped as its own binary but sits on no request path and is not part of a `memory` deployment.
> It depends on C1 only, through the public `MemoryStore` trait and `Migrator`; it adds no code to
> C7, C2, or either shipped binary, so NFR-MA3 (single-binary deployment, ADR-009) is unchanged. C11
> is build-order phase 7 because it measures the assembled store and must not be a dependency of
> anything that ships. Those properties are SA-BENCH-01 and are unchanged.
>
> **What it measures is not what this note first said.** As first specified C11 provisioned
> populations of 10,000 and 50,000 namespaces and reported a per-namespace footprint and a
> tenant-sweep cost (SA-FOOTPRINT-01, SA-SWEEP-COST-01) — a topology RFC 03 moves away from, and a
> sweep problem it calls absent at ten namespaces. Those legs are **withdrawn**. C11 now runs three
> legs against an embedded SurrealKV rig holding one deliberately-shaped corpus: relevance across
> co-resident users, absolute stage-1 vector latency across visible fractions with the crossover
> recorded as an interval, and per-user latency as the namespace grows under a pinned visible set.
> The three safety gates (SA-BENCH-ISOLATION-01, SA-BENCH-ISOLATION-02) and the `render_up` widening
> (SA-BENCH-RENDER-01) carry over unchanged; the report keeps its shape and its scheme-only URL rule.
> `SA-FOOTPRINT-01` and `SA-SWEEP-COST-01` still stand in `phase-1-analysis.md` as definitions but
> have no implementing leg, and `components/tenancy-benchmark.md` keeps the withdrawn procedures under
> a heading that marks them unimplemented. Details and the current legs are in that component spec.

**Dependency DAG (build order):**

```
C1 Memory Store
 ├─> C2 Work Queue ─┐
 │                  ├─> C4 Write Pipeline
 │                  └─> C7 Maintenance
 ├──────────────────────────────────────> C6 Retrieval ─┐
 └───────────────────────────────────────────────────┐  │
C3 Auth & Scope ──────────────────────────────────┐  │  │
                                                   ▼  ▼  ▼
                                              C9 Service Layer ─┬─> C8 HTTP API Edge (bin recall)
                                                                └─> C10 MCP API Edge (bin memory-mcp)

C1 Memory Store ─────────────────────────────────> C11 Tenancy Benchmark (bin memory-bench)
```

No cycles: every edge points from a lower phase to a higher one. C6 Retrieval depends only on C1
(C5 Freshness Checker is retired — ADR-014). The Service Layer (C9) reaches the Write Pipeline only
indirectly, by enqueuing on C2 — the write path is asynchronous (ADR-004), so there is no synchronous
C9→C4 edge. Both edges (C8, C10) depend only on C9; they share no code beyond it (SA-SVC-01). C11 is a leaf: it depends on C1 and nothing depends on it, so adding it
cannot introduce a cycle and cannot change the build order of anything that ships.

**External provider adapters** (`EmbeddingClient`, `RerankClient`,
`PiiDetector`) are not separate components — they are trait abstractions defined in 2C and injected
into the components that use them. They live under `src/providers/` as thin HTTP adapters and carry no
domain logic, so they need no standalone Phase 3 spec; their contracts are the traits in 2C.

---

## 2C — Shared Types Catalogue

Every type used by more than one component. Defined once here; **duplicated into the Shared Context
of each consuming spec** (never cross-referenced). Rust definitions with `serde` (JSON) and SurrealDB
mapping notes. All public API JSON uses `snake_case` field names.

### 2C.1 — Response envelopes

```rust
// src/types/envelope.rs   Used by: C8 (every handler); shape mirrored by all error producers.

/// Success envelope. Every 2xx body is exactly this shape.
#[derive(Serialize)]
pub struct Success<T: Serialize> {
    pub data: T,
    pub meta: Meta,
}

#[derive(Serialize, Default)]
pub struct Meta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,   // opaque pagination cursor (SA-PAGE-01)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstained: Option<bool>,       // true on a recall that gated out all candidates
    pub correlation_id: String,        // uuid, echoes the per-request id
}

/// Error envelope. Every non-2xx body is exactly this shape.
#[derive(Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub code: String,        // SCREAMING_SNAKE_CASE, from the Phase 4 error registry (SA-ENV-02)
    pub message: String,     // human-readable, never leaks internal state or PII
    pub correlation_id: String,
}
```

**Validation:** `code` MUST be a registered value (Phase 4 Error Handling). `correlation_id` MUST be
present on every response. **Used by:** C8 (emits), all components (produce typed errors mapped here).

### 2C.2 — Domain entities

```rust
// src/types/domain.rs   Used by: C1, C4, C6, C7 (and C8 for GET fact).

#[derive(Serialize, Deserialize, Clone)]
pub struct Fact {
    pub id: String,                       // "fact:<uuidv7>" (SA-ID-01)
    pub content: serde_json::Value,       // structured assertion (object), not free text
    pub entities: Vec<String>,            // entity ids this fact connects (>=1)
    pub source_id: Option<String>,        // "source:<uuidv7>" provenance, null for agent-stated
    pub memory_class: MemoryClass,        // memory's OWN vocabulary — unrelated to fact_class
    /// The caller-owned class this fact was **authorised as** (SA-AUTHZ-09). Opaque to `memory`.
    /// `None` = written without a class. `#[serde(default)]` so rows predating the field decode.
    /// Distinct from `memory_class`, which is memory's episodic/semantic/consolidated vocabulary.
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
    /// Collection label (ADR-020, SA-COLL-01); "" = the default collection. Added January 2026.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Entity {
    pub id: String,                       // "entity:<uuidv7>"
    pub canonical_name: String,           // 1..=512 chars, non-empty
    pub aliases: Vec<String>,
    pub owner: ScopeRef,
    /// Collection label (ADR-020, SA-COLL-01); "" = the default collection. Added January 2026.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,
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
    /// Collection label (ADR-020, SA-COLL-01); "" = the default collection. Added January 2026.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,
    /// The relationship a person's correction replaced (`Service::correct_link`; ADR-021, SA-GRAPH-06).
    /// Absent on every relationship an agent's write asserted. Added January 2026.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Source {
    pub id: String,                       // "source:<uuidv7>"
    pub origin_ref: String,               // document/system handle, opaque to recall
    pub modification_marker: Option<String>, // ETag / Last-Modified token for freshness
    pub trust_signal: f64,                // [0,1] prior trust of this source
    pub owner: ScopeRef,
    /// Collection label (ADR-020, SA-COLL-01); "" = the default collection. Added January 2026.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryClass { Episodic, Semantic, Consolidated }   // procedural rejected (SA-CLASS-01)

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility { UserPrivate, TeamShared, TenantShared } // (SA-VIS-01)
```

**Validation:** `entities.len() >= 1`; all score fields in `[0,1]`; `valid_to` (if present) `>=
valid_from`; `content` MUST be a JSON object. **SurrealDB mapping:** one table per type within the
tenant namespace (`fact`, `entity`, `relationship`, `source`); `entities`/`from`/`to` are SurrealDB
record links; `content` is a `flexible` object; a per-table HNSW vector index on the fact embedding
(2C.6) and a BM25 keyword index on serialised `content`. The persisted `fact` **row** additionally
carries two derived, persistence-only fields not part of the JSON `Fact` contract — `embedding:
option<array<float>>` (the fact-content vector for the HNSW index, written by C4, refreshed by C7) and
`embedding_model: option<string>` (the embedding-model version tag the C7 re-embed scan keys on);
these never appear in an API `Fact` payload. *Added January 2026 (ADR-021):* the persisted `entity` row
carries the persistence-only field `names_norm: array<string>` (the normalised canonical name and
aliases, SA-GRAPH-01) that entity matching reads, and a `relationship_fact` table records which facts
asserted or ended each relationship (`LinkRole`, 2C.6); neither appears in an API payload. **Used by:** C1 (persists), C4 (writes),
C6 (reads/ranks), C7 (maintains), C8 (GET fact).

### 2C.3 — Scope & auth context

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
    /// Calling service / client identity, from the token's `azp` (or the configured claim).
    /// `None` when the token carries none — never substituted with `user` (SA-AUTHZ-08).
    pub client: Option<String>,
    /// Which fact classes this token may assert. `None` = the claim was absent = every class
    /// (SA-AUTHZ-02). `Some(empty)` = no class may be asserted, which is a different statement.
    pub fact_authority: Option<Vec<FactAuthority>>,
    pub correlation_id: String,
}

#[derive(Clone, Copy)]
pub struct OpSet { pub read: bool, pub write: bool, pub forget: bool }

/// One entry of the `memory_fact_authority` claim: a class this token may assert, and optionally the
/// single source it may attribute that class to. Both are **opaque strings** owned by the caller's
/// vocabulary; `memory` compares and never interprets (SA-AUTHZ-01, SA-LLMFREE-01).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FactAuthority {
    pub class: String,             // non-empty
    pub source: Option<String>,    // non-empty when present; binds to RememberRequest.source.origin_ref
}
```

**Fact-class authority rule (binding, applied once in C9 `remember`, SA-AUTHZ-01):** a write naming
`fact_class = Some(c)` is admitted iff `ctx.fact_authority` is `None` (no claim — every class, SA-AUTHZ-02),
**or** it contains an entry whose `class == c` **and** whose `source` is either `None` or equal to the
request's `source.origin_ref` (SA-AUTHZ-05). A write naming no `fact_class` asserts no class and is
admitted on the `memory.write` scope alone. The rule does not apply to `retire` or `delete` (SA-AUTHZ-07).

**Read filter rule (binding for every store query):** a caller may read a Fact/Entity/Relationship
iff `record.owner.tenant == ctx.tenant` **and** ( `record.owner.user == ctx.user`
**or** (`record.visibility == TeamShared` and `record.owner.team ∈ ctx.teams`)
**or** `record.visibility == TenantShared` **or** the caller holds an **active consent grant** over that Fact — one issued by its owner, naming the caller as grantee, unrevoked, and inside its validity window (`docs/spec/components/consent-grants.md`, SA-GRANT-01). The grant arm applies to Facts only, and never to the vector-search path (SA-GRANT-08). ). Cross-tenant access is structurally impossible — a
different tenant is a different namespace. **Used by:** C3 (builds `ScopeContext`), all data
components (apply the filter), C9 (Consent Grants, issues and revokes the fourth arm).

### 2C.4 — API request/response payloads

```rust
// src/types/api.rs   Used by: C8 (HTTP), C6 (recall), C4 (remember).

#[derive(Deserialize)]
pub struct RecallRequest {
    #[serde(default)]
    pub query: String,                       // 1..=4096 chars; exactly one of a non-empty query and `searches` (SA-SUBQ-01, January 2026)
    #[serde(default)]
    pub searches: Option<Vec<SubQuery>>,     // 1..=4 typed sub-queries, the first weighted 2.0 (ADR-020)
    #[serde(default)]
    pub filters: RecallFilters,
    #[serde(default = "default_result_cap")]
    pub result_cap: u8,                      // [1,50], default 10 (SA-CAP-01)
    #[serde(default)]
    pub cursor: Option<String>,              // opaque, from a prior meta.next_cursor
    #[serde(default)]
    pub include_provenance: bool,            // opt-in: attach source origin_ref + marker (SA-PROV-01)
}

#[derive(Deserialize, Default)]
pub struct RecallFilters {
    pub memory_class: Option<MemoryClass>,
    pub visibility: Option<Visibility>,
    pub entity: Option<String>,              // restrict to facts touching this entity id
    pub valid_at: Option<DateTime<Utc>>,     // as-of query into the bi-temporal history
    #[serde(default)]
    pub collections: Option<Vec<String>>,    // 1..=32 distinct labels (SA-COLL-01), applied before every signal (ADR-020)
}

#[derive(Deserialize, Clone)]
pub struct SubQuery {
    #[serde(rename = "type")]
    pub kind: SubQueryKind,
    pub query: String,                       // 1..=4096 chars
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubQueryKind { Lex, Vec }

#[derive(Serialize)]
pub struct RecallResponse {
    pub facts: Vec<RankedFact>,              // wrapped in Success<RecallResponse>
    /// The live links recall followed to reach a returned fact; ≤ 20 (ADR-021, January 2026).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<RecallLink>,
}

#[derive(Serialize)]
pub struct RecallLink {
    pub relationship: Relationship,
    pub from_name: String,                   // canonical name of `relationship.from`
    pub to_name: String,                     // canonical name of `relationship.to`
    pub reached: Vec<String>,                // fact ids on this page the link led to
}

#[derive(Serialize)]
pub struct RankedFact {
    pub fact: Fact,
    pub score: f64,                          // final ranking score [0,1]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceProvenance>,    // present only when include_provenance and fact has a source (SA-PROV-01)
    pub fading: bool,                        // memory::maintenance::is_fading (SA-GRAPH-07; ADR-021, January 2026)
}

/// Returned per sourced fact when the recall request opts in (SA-PROV-01, ADR-014). Lets the agent
/// run its own source-freshness check; `memory` performs no check and asserts no currency.
#[derive(Serialize)]
pub struct SourceProvenance {
    pub origin_ref: String,                  // document/system handle the agent resolves
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modification_marker: Option<String>, // ETag / Last-Modified token captured at write time
}

#[derive(Deserialize)]
pub struct RememberRequest {
    pub content: serde_json::Value,          // structured assertion object (the agent extracts; ADR-015)
    pub source: Option<SourceInput>,
    #[serde(default)]
    pub memory_class: Option<MemoryClass>,   // optional; default Episodic (SA-WRITE-STRUCTURED-01)
    /// Optional caller-owned label naming the class of assertion being made. Opaque to `memory`:
    /// it is compared against the token's authority claim and never interpreted (SA-AUTHZ-01).
    /// Absent = an unclassed write, admitted on the `memory.write` scope alone.
    /// 1..=128 chars when present; an empty string is VAL_INVALID_BODY.
    #[serde(default)]
    pub fact_class: Option<String>,
    /// Collection label (SA-COLL-01); absent = "". Carried onto the fact and any entity or source the
    /// write creates (ADR-020, January 2026).
    #[serde(default)]
    pub collection: Option<String>,
    /// Entities the fact is about, resolved by C4's three tiers; aliases join the resolved entity's
    /// aliases. 0..=16 (ADR-021, SA-GRAPH-01, January 2026).
    #[serde(default)]
    pub entities: Vec<EntityAssertion>,
    /// Links the agent states between named entities; 0..=16 (ADR-021, SA-GRAPH-02, January 2026).
    #[serde(default)]
    pub links: Vec<LinkAssertion>,
    /// Facts a consolidated fact summarises: 0, or 2..=64 "fact:<uuid>" ids readable by the caller and
    /// currently valid; non-empty only with memory_class = Consolidated (ADR-021, January 2026).
    #[serde(default)]
    pub derived_from: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct EntityAssertion {
    pub name: String,                        // 1..=512 chars after NFC + trim
    #[serde(default)]
    pub aliases: Vec<String>,                // 0..=32, each 1..=512 chars
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct LinkAssertion {
    pub from: String,                        // a name or alias named in this request (entities[], content.subject, content.object)
    pub kind: String,                        // 1..=128 chars after normalisation (lower-case, trim, collapse whitespace)
    pub to: String,                          // as `from`
    #[serde(default)]
    pub ended: bool,                         // true = close the live link; assert nothing
    #[serde(default)]
    pub confidence: Option<f64>,             // [0,1], default 1.0
}

/// ADR-021 read operations (any client) and person operations (ordin-in-process only, SA-GRAPH-09).
/// Used by: C9, C8 (the read routes). Added January 2026.
#[derive(Deserialize)] pub struct FactHistoryRequest { pub id: String }
#[derive(Deserialize)] pub struct FactOriginsRequest { pub id: String }
#[derive(Deserialize)] pub struct ListEntitiesRequest { #[serde(default)] pub search: Option<String> /* 1..=200 */, #[serde(default)] pub cursor: Option<String> }
#[derive(Deserialize)] pub struct EntityDetailRequest { pub id: String }
#[derive(Deserialize)] pub struct CorrectLinkRequest { pub id: String, pub kind: String /* 1..=128 */, pub from: String /* entity id */, pub to: String /* entity id */ }
#[derive(Serialize)] pub struct FactHistory { pub versions: Vec<FactVersion> }          // oldest first, ≤ 50
#[derive(Serialize)] pub struct FactVersion { pub fact: Fact, pub links: Vec<(Relationship, LinkRole)> }
#[derive(Serialize)] pub struct OriginFact { pub id: String, pub fact: Option<Fact>, pub ended: bool }  // fact None = not readable or gone

/// ordin-in-process only (SA-SVC-CHECK-01; ADR-020, January 2026). Used by: C9.
#[derive(Deserialize)]
pub struct CheckContentRequest { pub text: String }          // 1..=8000 chars

#[derive(Serialize)]
pub struct ContentCheck { pub personal_data: PersonalDataFinding, pub instruction_like: bool }

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersonalDataFinding { None, Review, Redact }

#[derive(Deserialize)]
pub struct SourceInput {
    pub origin_ref: String,
    pub modification_marker: Option<String>,
}

#[derive(Serialize)]
pub struct WriteAck { pub job_id: String, pub status: JobAckStatus }  // async (ADR-004)

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum JobAckStatus { Accepted, AlreadyAccepted }   // AlreadyAccepted => idempotent replay

#[derive(Serialize)]
pub struct DeletionProof {
    pub deleted_at: DateTime<Utc>,
    pub record_id: String,
    pub derived_removed: Vec<String>,        // ids of removed derived summaries
    pub embeddings_removed: u32,
    pub digest: String,                      // sha256 hex over sorted removed ids (SA-DELETE-01)
}

fn default_result_cap() -> u8 { 10 }
```

**Used by:** C8 (deserialises requests, serialises responses), C6 (`RecallRequest`/`RankedFact`), C4
(`RememberRequest`), C7+C8 (`DeletionProof`).

### 2C.5 — Work queue types

```rust
// src/types/job.rs   Used by: C2 (transport), C4/C5/C7/C8 (produce/consume).

#[derive(Serialize, Deserialize, Clone)]
pub struct WorkJob {
    pub id: String,                          // "work_job:<uuidv7>"
    pub kind: JobKind,
    pub payload: serde_json::Value,          // kind-specific, validated by the consumer
    pub scope: ScopeRef,
    pub idempotency_key: Option<String>,     // present for API-originated writes
    pub attempts: u32,
    pub status: JobStatus,
    pub not_before: DateTime<Utc>,           // backoff schedule (SA-QUEUE-02)
    pub created_at: DateTime<Utc>,
    pub leased_until: Option<DateTime<Utc>>, // claim lease; null when unclaimed
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobKind { ExtractFact, ReEmbedFact, HardDelete }  // ReReadSource removed (ADR-014); Consolidate removed (ADR-015)

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus { Pending, Leased, Done, DeadLetter }
```

**Used by:** C2 (owns the table + claim/lease), C4 (consumes `ExtractFact`), C7 (consumes
`ReEmbedFact`/`HardDelete`), C8 (produces `ExtractFact`, `HardDelete`).

### 2C.6 — Provider & infrastructure traits (dependency-injected)

```rust
// src/types/ports.rs   The seams that keep providers swappable (OQ-MODELS/OQ-QUEUE/OQ-STORE).

/// Added January 2026 (ADR-020). Implemented by C1's store and by ordin's SQL storage (change M-2),
/// whose `StoragePort` supertrait includes it. Caller: ordin's Memory Service only; no edge route.
#[async_trait]
pub trait CollectionErasure: Send + Sync {
    async fn erase_collection(&self, tenant: &str, team: &str, collection: &str)
        -> Result<TenantErasureProof, StoreError>;   // predicate delete of one collection, verified by re-count
}

/// Added January 2026 (ADR-021). Implemented by C1's store and by ordin's SQL storage (change M-9),
/// whose `StoragePort` supertrait includes it. Full contract: `components/memory-store.md`, amendment
/// January 2026 (entities, links and graph recall).
#[async_trait]
pub trait GraphStore: Send + Sync {
    async fn find_live_relationship(&self, ctx: &ScopeContext, from: &str, kind: &str, to: &str) -> Result<Option<Relationship>, StoreError>;
    async fn relationship_kinds(&self, ctx: &ScopeContext, limit: u32) -> Result<Vec<String>, StoreError>;
    async fn link_fact(&self, ctx: &ScopeContext, relationship_id: &str, fact_id: &str, role: LinkRole) -> Result<(), StoreError>;
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
    async fn backfill_entity_names(&self, tenant: &str, batch: u32) -> Result<u64, StoreError>;   // idempotent; 0 = done
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkRole { Asserted, Ended }
pub struct Neighbour { pub entity_id: String, pub hop: u8, pub via: String /* relationship id */ }
#[derive(Serialize)] pub struct EntitySummary { pub entity: Entity, pub fact_count: u32, pub link_count: u32 }
#[derive(Serialize)] pub struct EntityPage { pub items: Vec<EntitySummary>, pub next_cursor: Option<String> }   // by canonical_name, then id; 100 a page
#[derive(Serialize)] pub struct EntityDetail { pub entity: Entity, pub facts: Vec<Fact>, pub links: Vec<Relationship>, pub names: HashMap<String, String> }
#[derive(Serialize)] pub struct EntityCount { pub facts: u32, pub links: u32 }
#[derive(Serialize)] pub struct EntityErasureProof { pub erased_at: DateTime<Utc>, pub entity_id: String, pub facts_removed: u32,
                                                     pub links_removed: u32, pub derived_removed: u32, pub digest: String }

#[async_trait]
pub trait MemoryStore: Send + Sync {                 // impl: embedded SurrealDB (C1)
    // --- Fact CRUD + bi-temporal ---
    async fn put_fact(&self, f: &Fact) -> Result<(), StoreError>;        // upsert; embedding passed alongside
    async fn get_fact(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Fact>, StoreError>;
    async fn recall(&self, ctx: &ScopeContext, q: &StageOneQuery)
        -> Result<Vec<Candidate>, StoreError>;                          // multi-signal stage-1
    async fn end_validity(&self, ctx: &ScopeContext, id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;
    async fn supersede(&self, ctx: &ScopeContext, old_id: &str, new_id: &str, at: DateTime<Utc>)
        -> Result<(), StoreError>;
    async fn hard_delete(&self, ctx: &ScopeContext, id: &str) -> Result<DeletionProof, StoreError>;
    // --- Entity CRUD + resolution support ---
    async fn put_entity(&self, e: &Entity) -> Result<(), StoreError>;    // upsert
    async fn get_entity(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Entity>, StoreError>;
    async fn find_entity_by_name(&self, ctx: &ScopeContext, name: &str) -> Result<Vec<Entity>, StoreError>;
    async fn merge_entities(&self, ctx: &ScopeContext, keep_id: &str, merge_id: &str) -> Result<(), StoreError>;
    // --- Relationship CRUD ---
    async fn put_relationship(&self, r: &Relationship) -> Result<(), StoreError>;
    async fn get_relationship(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Relationship>, StoreError>;
    async fn end_relationship_validity(&self, ctx: &ScopeContext, id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;
    // --- Source CRUD ---
    async fn put_source(&self, s: &Source) -> Result<(), StoreError>;    // upsert
    async fn get_source(&self, ctx: &ScopeContext, id: &str) -> Result<Option<Source>, StoreError>;
    // --- Audit (SA-AUDIT-01) ---
    async fn append_audit(&self, e: &AuditEntry) -> Result<(), StoreError>;   // append-only, synchronous
    // --- Maintenance surface consumed by C7 (all take ctx; ctx.tenant selects the namespace) ---
    async fn list_tenants(&self) -> Result<Vec<String>, StoreError>;          // admin op, no ctx
    async fn scan_recent_episodes(&self, ctx: &ScopeContext, since: DateTime<Utc>, limit: u32)
        -> Result<Vec<Fact>, StoreError>;
    async fn scan_contradiction_candidates(&self, ctx: &ScopeContext, limit: u32)
        -> Result<Vec<(Fact, Fact)>, StoreError>;
    async fn scan_decay_candidates(&self, ctx: &ScopeContext, salience_floor: f64, limit: u32)
        -> Result<Vec<Fact>, StoreError>;                                    // coarse prefilter; C7 owns the decay maths
    async fn scan_reembed_candidates(&self, ctx: &ScopeContext, current_model_version: &str, limit: u32)
        -> Result<Vec<Fact>, StoreError>;
    async fn update_fact_maintenance_fields(&self, ctx: &ScopeContext, f: &Fact) -> Result<(), StoreError>;
    async fn set_fact_embedding(&self, ctx: &ScopeContext, fact_id: &str, vector: &[f32], model_version: &str)
        -> Result<(), StoreError>;
    // --- Lifecycle / tenancy ---
    async fn ensure_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>;   // idempotent
    async fn erase_tenant(&self, tenant: &str) -> Result<TenantErasureProof, StoreError>; // ADR-018 erasure
    async fn drop_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>;      // ADR-011, single-tenant ns only
    async fn ready(&self) -> Result<(), StoreError>;   // connection live + vector-index dim == MEMORY_EMBED_DIM
}

// (Freshness tagging removed by ADR-014 — `memory` performs no source-change check; the agent does.)

#[async_trait]
pub trait WorkQueue: Send + Sync {                    // impl: store-backed (C2)
    async fn enqueue(&self, job: WorkJob) -> Result<String, QueueError>; // returns job id
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
pub trait RerankClient: Send + Sync {                 // impl: HTTP adapter
    async fn rerank(&self, query: &str, docs: &[String]) -> Result<Vec<f64>, ProviderError>;
}

// (LlmClient removed by ADR-015 — recall is LLM-free: the agent extracts and consolidates; recall
// accepts structured agent-asserted facts. `ExtractedFact`/`EntityMention` remain as the C4-internal
// wrapper for structured content. `InsightCandidate` is removed with server-side consolidation.)

// (BrokerClient removed by ADR-014 — `memory` makes no outbound broker call.)

#[async_trait]
pub trait PiiDetector: Send + Sync {                  // impl: model/heuristic adapter
    async fn scan(&self, content: &serde_json::Value) -> Result<Vec<PiiSpan>, ProviderError>;
}
```

The three error/result types shared across every provider trait are defined **here** (one canonical
home), so all adapters share them:

```rust
// src/types/ports.rs (continued)

#[derive(thiserror::Error, Debug)]
pub enum ProviderError {                  // shared by EmbeddingClient/RerankClient/PiiDetector
    #[error("provider timeout")]          Timeout,            // -> 504 PROVIDER_TIMEOUT
    #[error("provider status {0}")]       Status(u16),        // -> 502 PROVIDER_ERROR
    #[error("provider transport: {0}")]   Transport(String),  // -> 502 PROVIDER_ERROR
    #[error("provider malformed: {0}")]   Malformed(String),  // -> 502 PROVIDER_ERROR
}

pub struct PiiSpan {                       // PiiDetector::scan result (C4)
    pub json_pointer: String,              // RFC 6901 pointer into `content`
    pub pii_type: String,                  // e.g. "email", "person", "phone"
    pub confidence: f64,                   // [0,1]
}
```

**Used by:** the trait is the contract; each consuming component spec (C1/C2/C4/C6/C7) duplicates
the exact signatures it depends on into its Shared Context. `StoreError` and the C1-owned
`Candidate`/`StageOneQuery`/`AuditEntry` are defined in full in the **C1 Memory Store** spec;
`QueueError` in **C2**; `ExtractedFact`/`EntityMention` in **C4** — each referenced by name here.
(`InsightCandidate` removed with server-side consolidation, ADR-015.) The full `MemoryStore` surface
above is canonical; C1 implements it exactly.

### 2C.7 — Typed application error

```rust
// src/error.rs   Used by: every component (produces) and C8 (maps to HTTP + ErrorBody).

/// Discriminator for the VAL_* family carried by AppError::Validation (the precise code is selected
/// by this kind, so one variant covers the whole VAL_* family).
pub enum ValidationKind { InvalidBody, OutOfRange, UnsupportedClass, MissingIdempotencyKey, BodyTooLarge }

/// Discriminator carried by AppError::Unauthenticated (selects AUTH_MISSING_TOKEN vs AUTH_INVALID_TOKEN).
pub enum AuthKind { Missing, Invalid }

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("validation: {1}")]      Validation(ValidationKind, String),  // -> 400 VAL_* (code per kind)
    #[error("unauthenticated: {1}")] Unauthenticated(AuthKind, String),   // -> 401 AUTH_* (code per kind)
    #[error("forbidden: {0}")]       Forbidden(String),                   // -> 403 SCOPE_FORBIDDEN
    #[error("insufficient scope: {0}")] InsufficientScope(String),        // -> 403 AUTH_INSUFFICIENT_SCOPE
    #[error("not found")]            NotFound,                            // -> 404 NOT_FOUND
    #[error("rate limited")]         RateLimited,                         // -> 429 RATE_LIMITED
    #[error("store: {0}")]           Store(#[from] StoreError),           // -> 503 STORE_UNAVAILABLE / 504 STORE_TIMEOUT
    #[error("queue: {0}")]           Queue(#[from] QueueError),           // -> 503 QUEUE_UNAVAILABLE
    #[error("provider: {0}")]        Provider(#[from] ProviderError),     // -> 502/504 by kind
    #[error("internal")]             Internal,                            // -> 500 INTERNAL
}
```

The exact `AppError` → (HTTP status, `code`) mapping is the single registry in the Phase 4 Error
Handling spec. **Used by:** all components produce `AppError`; C8 owns the mapping.

---

## 2D — Configuration & Environment Variables

All configuration is loaded at startup with precedence **env var > config file > built-in default**
(Phase 4 Configuration spec). Secrets are read from env only and never logged. Startup fails fast on
a missing required value or a failed validation (e.g. embedding-dimension mismatch, SA-EMBED-01).

| Variable | Type | Default | Required | Owner Component | Description |
|---|---|---|---|---|---|
| `MEMORY_HTTP_ADDR` | socket addr | `0.0.0.0:8080` | no | C8 | Bind address for the HTTP API. |
| `MEMORY_STORE_PATH` | path | `./data/recall.db` | no | C1 | Embedded SurrealDB data directory (SurrealKV/RocksDB). |
| `MEMORY_STORE_REMOTE_URL` | url | _(unset)_ | no | C1 | If set, target a remote SurrealDB/TiKV cluster instead of embedding (ADR-009 scale-out). |
| `MEMORY_STORE_REMOTE_USER` | string | _(unset)_ | no | C1 | Root username for a **secured** remote store; signin fires only when both user and pass are set (FU-019). Setting exactly one of user/pass fails startup. |
| `MEMORY_STORE_REMOTE_PASS` | secret | _(unset)_ | no | C1 | Root password for a secured remote store (env only, redacted in logs). |
| `MEMORY_STORE_BACKEND` | enum `surrealkv\|rocksdb` | `surrealkv` | no | C1 | Embedded storage backend. |
| `MEMORY_OIDC_ISSUER` | url | _(none)_ | **yes** | C3 | OIDC issuer; discovery at `<issuer>/.well-known/openid-configuration`. |
| `MEMORY_OIDC_AUDIENCE` | string | _(none)_ | **yes** | C3 | Expected `aud` claim. |
| `MEMORY_OIDC_SUBJECT_CLAIM` | string | `sub` | no | C3 | Claim mapped to the user id. |
| `MEMORY_OIDC_TEAMS_CLAIM` | string | `groups` | no | C3 | Claim carrying team membership. |
| `MEMORY_OIDC_TENANT_CLAIM` | string | `tenant` | no | C3 | Claim carrying the tenant id. |
| `MEMORY_OIDC_CLIENT_CLAIM` | string | `azp` | no | C3 | Claim carrying the calling service / client identity, surfaced as `ScopeContext.client` and written to the audit trail. A token without it yields `None` (SA-AUTHZ-08). |
| `MEMORY_OIDC_AUTHORITY_CLAIM` | string | `memory_fact_authority` | no | C3 | Claim carrying write authority by fact class. Read **strictly**: present but unparseable is `AUTH_INVALID_TOKEN`, never ignored (SA-AUTHZ-03). Absent grants every class (SA-AUTHZ-02). |
| `MEMORY_JWKS_REFRESH_SECS` | u32 | `3600` | no | C3 | JWKS background refresh interval (SA-JWKS-01). |
| `MEMORY_EMBED_URL` | url | _(none)_ | **yes** | C4/C6 | Embedding provider endpoint. |
| `MEMORY_EMBED_API_KEY` | secret | _(none)_ | **yes** | C4/C6 | Embedding provider key (env only). |
| `MEMORY_EMBED_DIM` | u32 | `1024` | no | C1/C4/C6 | Embedding dimension; must equal the vector-index dimension (SA-EMBED-01). |
| `MEMORY_RERANK_URL` | url | _(none)_ | **yes** | C6 | Cross-encoder reranker endpoint. |
| `MEMORY_RERANK_API_KEY` | secret | _(none)_ | **yes** | C6 | Reranker key (env only). |
| `MEMORY_QUEUE_BACKEND` | enum `store\|nats` | `store` | no | C2 | Work-queue backend (SA-QUEUE-01). |
| `MEMORY_QUEUE_NATS_URL` | url | _(unset)_ | conditional | C2 | Required iff `MEMORY_QUEUE_BACKEND=nats`. |
| `MEMORY_JOB_MAX_ATTEMPTS` | u32 | `5` | no | C2 | Retry cap before dead-letter (SA-QUEUE-02). |
| `MEMORY_JOB_BACKOFF_BASE_MS` | u32 | `2000` | no | C2 | Backoff base for job retries. |
| `MEMORY_RESULT_CAP_MAX` | u8 | `50` | no | C6 | Hard upper bound on `result_cap` (SA-CAP-01). |
| `MEMORY_STAGE1_K` | u16 | `50` | no | C6 | Stage-1 candidate count fed to rerank (SA-RERANK-01). |
| `MEMORY_ABSTAIN_THRESHOLD` | f64 | `0.2` | no | C6 | Score below which recall abstains (SA-GATE-01). |
| `MEMORY_RECENCY_WEIGHT` | f64 | `0.15` | no | C6 | Recency boost weight `w` (SA-RECENCY-01). |
| `MEMORY_RECENCY_TAU_DAYS` | f64 | `30` | no | C6 | Recency decay constant `τ`. |
| `MEMORY_TRUST_ADMIT` | f64 | `0.7` | no | C4 | Write-gate admit threshold (SA-WGATE-01). |
| `MEMORY_TRUST_QUARANTINE` | f64 | `0.4` | no | C4 | Write-gate quarantine threshold. |
| `MEMORY_PII_REDACT_CONF` | f64 | `0.9` | no | C4 | PII redaction confidence threshold (SA-PII-01). |
| `MEMORY_SOURCE_TRUST_DEFAULT` | f64 | `0.5` | no | C4 | Prior trust assigned to a newly-seen source (write-gate scoring input). |
| `MEMORY_SALIENCE_FLOOR` | f64 | `0.3` | no | C7 | Decay salience floor (SA-DECAY-01). |
| `MEMORY_DECAY_K` | f64 | `10.0` | no | C7 | Global decay constant `k`. *Noted January 2026 (ADR-021):* `retrievability` divides elapsed **seconds** by `s·k`, so the default gives a time scale of 10 seconds, not the 10 days the C7 example reads; an embedding caller sets its own value (ordin sets `864000`, 10 days), and the default is unchanged here (HLD `10-risks.md`). |
| `MEMORY_PRUNE_RETRIEVABILITY` | f64 | `0.05` | no | C7 | Retrievability below which a low-salience fact is a prune candidate. |
| `MEMORY_IDLE_QUIET_SECS` | u32 | `300` | no | C7 | Idle period before a maintenance cycle (SA-MAINT-CADENCE-01). |
| `MEMORY_MAINT_MAX_INTERVAL_SECS` | u32 | `21600` | no | C7 | Hard fallback maintenance interval (6 h, SA-MAINT-CADENCE-01). Renamed from `MEMORY_CONSOLIDATE_MAX_INTERVAL_SECS` (ADR-015). |
| `MEMORY_IDEMPOTENCY_TTL_SECS` | u32 | `86400` | no | C8 | Idempotency-key retention window (SA-IDEM-01). |
| `MEMORY_RATE_READ_PER_MIN` | u32 | `120` | no | C8 | Read-class rate limit (SA-RATE-01). |
| `MEMORY_RATE_WRITE_PER_MIN` | u32 | `30` | no | C8 | Write-class rate limit. |
| `MEMORY_MAX_BODY_BYTES` | u32 | `1048576` | no | C8 | Max request body (1 MiB; input-validation spec). |
| `MEMORY_OTLP_ENDPOINT` | url | _(unset)_ | no | all | OpenTelemetry collector endpoint; observability disabled if unset. |
| `MEMORY_LOG_LEVEL` | enum | `info` | no | all | `error\|warn\|info\|debug\|trace`. |
| `MEMORY_ENV` | enum | `production` | no | all | `production\|development`; gates verbose error detail off in production. |
| `MEMORY_PROFILE` | enum `collective\|individual` | `collective` | no | C1, C6 | Which deployment scenario the instance serves. It is not a label: it selects the stage-1 vector strategy and the read filter together (`VectorStrategy::for_profile`, `store::vector_read_filter`), so `individual` pairs the narrow owner equality with the `fact_owner_user` index hint and `collective` runs the post-filtered index walk. An unknown value **fails startup** rather than defaulting, because silently serving the wrong scenario would mis-select the retrieval strategy and the topology together. `individual` is live from Stage 1 for the strategy only; its placement and multi-tenant namespace shape attach in Stage 2 and are blocked on RISK-017. |
| `MEMORY_CONFIG_FILE` | path | _(unset)_ | no | all | Optional configuration file, read **env-only** and forming the lowest-precedence layer beneath the env vars in this table — it is the "config file" of the `env var > config file > built-in default` order stated above, and the one key that cannot itself be set in that file. Absent or unreadable, every value falls to its env var or its default. |
| `MEMORY_QUEUE_REAPER_SECS` | u32 | `30` | no | C2 | Lease-reaper sweep cadence; expired leases revert to Pending (SA-QUEUE-02). |
| `MEMORY_QUEUE_POLL_MS` | u32 | `500` | no | C2 | Worker empty-claim poll interval (back-off between idle claims). |
| `MEMORY_REFORMULATION_ENABLED` | bool | `false` | no | C6 | A/B query-reformulation flag; off by default (good-mem §7.3, ADR-012). |
| `MEMORY_MCP_HTTP_ADDR` | socket addr | `0.0.0.0:8081` | no | C10 | Bind address for the MCP streamable-HTTP listener (the `memory-mcp` binary; SA-MCP-TRANSPORT-01). |
| `MEMORY_MCP_PATH` | path | `/mcp` | no | C10 | HTTP path the MCP endpoint is served at on the `memory-mcp` binary. |

> **Owner reassignment (ADR-016).** `MEMORY_IDEMPOTENCY_TTL_SECS`, `MEMORY_RATE_READ_PER_MIN`, and
> `MEMORY_RATE_WRITE_PER_MIN` move from owner **C8 → C9** — idempotency and rate limiting are now
> Service-Layer concerns applied uniformly to both edges (SA-SVC-01). `MEMORY_MAX_BODY_BYTES` stays an
> HTTP-transport concern and applies to both edge binaries (C8 and C10). All other variables are
> unchanged; both binaries read the same configuration model (SA-BIN-01).
| `MEMORY_EMBED_MODEL_VERSION` | string | `default` | no | C4/C6/C7 | Active embedding-model version tag; the C7 re-embed scan keys on it (SA-EMBED-01). |
| `MEMORY_MAINT_BATCH_SIZE` | u32 | `500` | no | C7 | Per-duty maintenance scan bound; keeps a cycle off the read-path store budget. |
| `MEMORY_REINFORCE_GAIN` | f64 | `0.5` | no | C7 | Stability gain `Δs` applied on recall/reinforcement (SA-DECAY-01). |
| `MEMORY_BENCH_STORE_URL` | url | _(unset)_ | **yes (C11 only)** | C11 | Endpoint of the **disposable** store the benchmark provisions against. `memory-bench` refuses to start when unset (SA-BENCH-ISOLATION-01); it never reads `MEMORY_STORE_PATH` or `MEMORY_STORE_REMOTE_URL`, so it cannot be pointed at a live deployment by inheriting its configuration. |
| `MEMORY_BENCH_POPULATION` | u32 list | `10000,50000` | no | C11 | Comma-separated tenant populations to provision and measure, in ascending order (acceptance criterion 1). |
| `MEMORY_BENCH_TENANT_PREFIX` | string | `bench` | no | C11 | Prefix for generated tenant ids; every generated id must satisfy `validate_tenant` before it reaches any DDL. |
| `MEMORY_BENCH_CONCURRENCY` | u32 | `16` | no | C11 | Maximum concurrent provisioning tasks. Bounds harness-side parallelism so the measured figure is store cost rather than harness contention. |
| `MEMORY_BENCH_ISOLATION` | enum | `namespace` | no | C11 | Which isolation shape to provision: `namespace` (one namespace per tenant, as built) or `database` (one database per tenant inside one namespace, the FU-006 mechanism). Settles OQ-NS-VS-DB by measurement. |
| `MEMORY_BENCH_REPORT_PATH` | path | `./bench-report.json` | no | C11 | Destination for the machine-readable run report. |
| `MEMORY_BENCH_SWEEP_SAMPLES` | u32 | `10` | no | C11 | Number of consecutive simulated scheduler ticks timed per population when measuring sweep cost (SA-SWEEP-COST-01). |
| `MEMORY_BENCH_RETAIN` | bool | `false` | no | C11 | When `false`, every namespace or database the run created is dropped before the harness exits. When `true`, the population is retained for the acceptance-criterion-7 latency leg. |
| `MEMORY_BENCH_COLLECTION_SIZES` | u32 list | `20000,200` | no | C11 | Sizes of the two fairness collections of the collection leg, large first (SA-BENCH-COLL-01; ADR-020, January 2026). |
| `MEMORY_BENCH_COLLECTION_LOAD` | u32 | `3` | no | C11 | Collections of the larger size for the collection leg's stage-1 timing run (4 sub-queries). |
| `MEMORY_BENCH_FIXTURE_DIR` | path | _(unset)_ | no | C11 | Directory of per-collection relevance fixtures (SA-BENCH-COLL-02); unset skips the fixture run. |
| `MEMORY_GRAPH_HOPS` | u8 | `1` | no | C1/C6 | Link-expansion depth in recall, `0..=2`; `0` turns the link list off (SA-GRAPH-05; ADR-021, January 2026). Out of range fails startup. |
| `MEMORY_GRAPH_FANOUT` | u32 | `32` | no | C1/C6 | Neighbour entities followed per hop, `1..=256` (ADR-021). |
| `MEMORY_GRAPH_CANDIDATE_CAP` | u32 | `200` | no | C1/C6 | Facts the graph and link lists together may contribute before fusion, `1..=1000` (ADR-021). |
| `MEMORY_TRUST_WEIGHT` | f64 | `0.1` | no | C6 | Weight of source trust in the final score, `0..=0.5`: `final × (1 + w·(trust − 0.5))` (SA-GRAPH-08; ADR-021). |
| `MEMORY_BENCH_GRAPH_SHAPE` | u32 list | `10000,20000,2000` | no | C11 | Facts, relationships and entities of the graph leg's corpus (SA-BENCH-GRAPH-01; ADR-021, January 2026). |

**Conditional requirements** are validated at startup: `MEMORY_QUEUE_NATS_URL` is required iff
`MEMORY_QUEUE_BACKEND=nats`; `MEMORY_STORE_REMOTE_URL` and `MEMORY_STORE_PATH` are mutually exclusive
(remote wins if both set, with a startup warning).
</content>
