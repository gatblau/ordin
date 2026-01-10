### SPEC: MCP API Edge
**File:** `src/mcp` (binary `memory-mcp`) | **Package:** `memory::mcp` | **Phase:** 6 | **Dependencies:** C9 (Service Layer)

> **Mode:** greenfield
> **derivedFromHld:** 0.12.0

#### Purpose

The MCP API Edge is the second externally-reachable surface of `recall` (ADR-016) — a thin transport adapter that exposes the same service operations as **MCP (Model Context Protocol) tools over streamable-HTTP**, shipped as a separate binary `memory-mcp`. It terminates the MCP streamable-HTTP transport, advertises the operation set through native MCP tool discovery (`tools/list`), dispatches each `tools/call` to the matching C9 Service Layer method, and renders the typed result or `AppError` as an MCP tool result or MCP error. It carries the same broker-injected `Authorization: Bearer <OIDC JWT>` as the REST edge and passes it straight to C9, so identity, scope, rate limiting, idempotency, and audit are exactly those of the REST edge — there is no second copy of that logic (SA-SVC-01). It holds no domain logic and no orchestration: every operation is a call into C9.

#### Approach

A small binary `memory-mcp` builds the same component state via the shared `build_state` and constructs a `Service` (C9), then serves an MCP server over streamable-HTTP using a maintained Rust MCP library (the exact crate is OQ-LIB, chosen at codegen; the library performs the MCP/JSON-RPC wire framing only). Each MCP tool is a thin closure: extract the bearer from the request's `Authorization` header, mint a correlation id, deserialise the tool arguments into the corresponding 2C request type, call the C9 method, and map the `CallResult`/`AppError` to a tool result/error. The chosen design is **one MCP tool per service operation** mirroring the REST routes, with tool input schemas **generated from the same 2C DTOs that back the OpenAPI document** — rejected alternative (a) hand-authored per-tool schemas (drifts from REST); rejected alternative (b) a single generic `call(operation, args)` tool (loses MCP's per-tool discovery and typed inputs). The edge owns no state and no tables; rate snapshots returned by C9 are surfaced as tool-result metadata where the MCP client can use them, but MCP defines no rate-limit headers, so a `RateLimited` error is rendered as an MCP error carrying the `RATE_LIMITED` code.

#### Shared Context

Duplicated from Phase 2C/2D and the C9 spec (implement from this section alone).

##### Service Layer interface consumed (C9)

```rust
pub struct CallContext<'a> { pub bearer: &'a str, pub correlation_id: &'a str, pub idempotency_key: Option<&'a str> }
pub struct CallResult<T> { pub data: T, pub rate: RateSnapshot, pub replayed: bool }
// Service methods: capabilities, recall, remember, get_fact, retire, delete — see the C9 spec.
```

`AppError` (2C.7) and its canonical registry `code`s are exactly as in the C9 / C8 specs. Request/response payloads (`RecallRequest`, `RememberRequest`, `RecallResponse`, `Fact`, `WriteAck`, `RetireAck`, `DeletionProof`, `Capabilities`) are the 2C.4 types, unchanged.

##### Configuration & environment variables (2D, edge-owned + consumed)

| Variable | Type | Default | Required | Description |
|---|---|---|---|---|
| `MEMORY_MCP_HTTP_ADDR` | socket addr | `0.0.0.0:8081` | no | Bind address for the MCP streamable-HTTP listener. |
| `MEMORY_MCP_PATH` | path | `/mcp` | no | HTTP path the MCP endpoint is served at. |
| `MEMORY_MAX_BODY_BYTES` | u32 | `1048576` | no | Max request body in bytes (1 MiB), applied at this HTTP transport as at the REST edge. |
| `MEMORY_OIDC_*`, `MEMORY_STORE_*`, `MEMORY_EMBED_*`, `MEMORY_RERANK_*`, `MEMORY_RATE_*`, `MEMORY_IDEMPOTENCY_TTL_SECS`, `MEMORY_ENV` | — | — | — | Read identically to the REST binary via the same `Config` (SA-BIN-01); both binaries share one configuration model. |

#### Public Interface

The MCP surface is the standard MCP methods over streamable-HTTP at `MEMORY_MCP_PATH`. Every call requires `Authorization: Bearer <OIDC JWT>` (the MCP transport is HTTP; the bearer rides the HTTP request).

**`tools/list`** — returns the tool catalogue: `recall`, `remember`, `get`, `retire`, `delete`, `capabilities`. Each tool entry carries a name, a one-line description, and a JSON-Schema `inputSchema` generated from the corresponding 2C request type. This is the MCP analogue of `GET /v1` + `/openapi.json`.

**`tools/call` — `recall`** — input: `RecallRequest` shape (`query`, optional `filters`, `result_cap`, `cursor`, `include_provenance`). Calls C9 `recall`. Result: the `RecallResponse` (ranked facts, each with `source` only when `include_provenance` was set) plus `abstained`/`next_cursor` surfaced in the tool-result structured content.

**`tools/call` — `remember`** — input: `RememberRequest` shape (`content` object, optional `source`, `memory_class`, optional `fact_class`) **plus a required `idempotency_key` string** (1–255 chars — the MCP analogue of the REST `Idempotency-Key` header). Calls C9 `remember`. Result: `WriteAck` (`job_id`, `status` = `accepted` | `already-accepted`). The `fact_class` label reaches C9 unchanged and is judged there against the token's authority claim, so the two edges give the same answer to the same write by construction rather than by two implementations agreeing (SA-AUTHZ-01, SA-SVC-01).

Every tool result carrying a `Fact` — `recall` and `get` — carries `fact_class` on it when the fact
was written with one, by the same mechanism as the REST edge: the field is on the shared 2C `Fact`
type both surfaces serialise, so the two cannot disagree about it (SA-AUTHZ-09).

**`tools/call` — `get`** — input: `{ id: string }`. Calls C9 `get_fact`. Result: the `Fact`. (Conditional-GET/ETag is a REST-only optimisation and is not offered over MCP.)

**`tools/call` — `retire`** — input: `{ id: string, idempotency_key: string }`. Calls C9 `retire`. Result: `RetireAck`.

**`tools/call` — `delete`** — input: `{ id: string, idempotency_key: string }`. Calls C9 `delete`. Result: `DeletionProof` (verifiable hard delete; unchanged semantics, SA-DELETE-01).

**`tools/call` — `capabilities`** — input: none. Calls C9 `capabilities`. Result: `Capabilities`.

The MCP server also implements the protocol's `initialize` handshake and serves only over `MEMORY_MCP_HTTP_ADDR`; no stdio transport is offered (ADR-016).

##### Example

`tools/call { "name": "recall", "arguments": {"query":"who owns the orders table","result_cap":5} }` with header `Authorization: Bearer eyJ…` → tool result with structured content `{"facts":[{"fact":{…},"score":0.87}],"abstained":false}`. A `tools/call` to `remember` without a valid bearer → an MCP error with `code` `AUTH_MISSING_TOKEN` / `AUTH_INVALID_TOKEN` (the same registry code the REST edge returns at 401).

#### Internal Logic

1. **Bootstrap.** `main` loads `Config` (X6), initialises observability (X3/X5), calls the shared `build_state(config)` to assemble the component stack, constructs `Service` (C9), registers the six tools, and serves the MCP server on `MEMORY_MCP_HTTP_ADDR` at `MEMORY_MCP_PATH`. Only `main` may panic on unrecoverable bootstrap (X1).
2. **Per `tools/call`:** (a) enforce the body-size limit (`MEMORY_MAX_BODY_BYTES`) → an oversize body is an MCP error carrying `VAL_BODY_TOO_LARGE`; (b) extract the bearer from the HTTP `Authorization` header (`""` if absent); (c) mint a correlation id (inbound `X-Correlation-Id` if a valid UUID, else a fresh UUIDv4); (d) deserialise the tool `arguments` into the 2C request type — a shape mismatch is an MCP error carrying `VAL_INVALID_BODY`; for writes, read the required `idempotency_key` argument; (e) build `CallContext{ bearer, correlation_id, idempotency_key }` and call the matching C9 method.
3. **Map the result.** `Ok(CallResult{ data, rate, replayed })` → an MCP tool result whose structured content is `data` serialised as JSON; the correlation id and `rate` (limit/remaining/reset) are attached as tool-result metadata. `Err(AppError)` → an MCP error whose payload carries the registry `code` (the same string the REST edge maps to a status) and the registry human `message` (production: fixed text; development: underlying detail appended), plus the correlation id. The classification is C9's `AppError`→`code` mapping reused verbatim — the edge invents no codes.
4. **`tools/list`** returns the six tool descriptors with `inputSchema` generated from the 2C request types (the same type definitions the OpenAPI document is built from), so the MCP and REST contracts cannot drift.
5. **No audit/auth/rate logic here.** Authentication, authorisation, rate limiting, idempotency, and the audit write all happen inside the C9 call (steps 1–6 of the C9 Internal Logic). The edge adds none of them.

#### Data Model

N/A — the MCP API Edge owns no tables and introduces no DDL. Idempotency records and the audit trail are written by C9 via C1 ports.

#### Error Table

The edge renders every C9 `AppError` as an MCP error carrying the **same registry `code`** the REST edge maps to the HTTP status shown (parity is the contract, SA-MCP-MAP-01). Plus two transport-local conditions:

| Condition | Equivalent HTTP status | Code | MCP rendering |
|-----------|------|------|---------------|
| Tool arguments fail to deserialise into the request type | 400 | VAL_INVALID_BODY | MCP error, `code` VAL_INVALID_BODY |
| Request body exceeds `MEMORY_MAX_BODY_BYTES` | 413 | VAL_BODY_TOO_LARGE | MCP error, `code` VAL_BODY_TOO_LARGE |
| Missing / invalid bearer | 401 | AUTH_MISSING_TOKEN / AUTH_INVALID_TOKEN | MCP error with the code (from C9) |
| Insufficient scope | 403 | AUTH_INSUFFICIENT_SCOPE | MCP error with the code (from C9) |
| `fact_class` not covered by the token's authority claim | 403 | AUTHZ_CLASS_FORBIDDEN | MCP error with the code (from C9, SA-AUTHZ-01) |
| Not found / out of scope | 404 | NOT_FOUND | MCP error with the code (from C9) |
| Missing `idempotency_key` argument on a write tool | 400 | VAL_MISSING_IDEMPOTENCY_KEY | MCP error with the code (from C9) |
| Rate-limit exhausted | 429 | RATE_LIMITED | MCP error with the code (no MCP rate headers exist) |
| Store/queue/provider failures | 502/503/504 | PROVIDER_* / STORE_* / QUEUE_* | MCP error with the code (from C9) |
| Unmapped internal failure | 500 | INTERNAL | MCP error, `code` INTERNAL |

Minimum-two satisfied; every code traces to the central registry via C9.

#### Acceptance Criteria (Gherkin)

```gherkin
Feature: MCP API Edge

  Scenario: Happy path — recall tool returns ranked facts via the Service Layer
    Given the memory-mcp server is running and a valid read-scoped bearer in the Authorization header
    And the store holds facts matching "who owns the orders table" in the caller's scope
    When the client sends tools/call name "recall" arguments {"query":"who owns the orders table","result_cap":5}
    Then the tool result structured content contains the ranked facts
    And exactly one audit record with operation "recall" outcome "success" was written by the Service Layer

  Scenario: Edge case — tools/list advertises the six tools with input schemas
    When the client sends tools/list
    Then the result lists tools recall, remember, get, retire, delete, capabilities
    And each tool carries an inputSchema generated from the same types that back the OpenAPI document

  Scenario: Edge case — idempotent remember replay over MCP returns the original ack
    Given remember was called with idempotency_key "k-001" one minute ago
    When the client calls the remember tool again with the same idempotency_key
    Then no new WorkJob is enqueued and the result status is "already-accepted"

  Scenario: Error path — missing bearer yields an MCP error with the same code as REST
    Given no Authorization header
    When the client sends tools/call name "recall"
    Then the response is an MCP error whose code is AUTH_MISSING_TOKEN
    And no audit record is written

  Scenario: Error path — error-code parity with the REST edge
    Given a fact id that does not exist in the caller's scope
    When the client calls the get tool for that id
    Then the MCP error code is NOT_FOUND, identical to the REST edge's 404 code for the same input

  Scenario: Error path — fact-class authority is enforced on the MCP edge too
    Given a write-scoped bearer whose memory_fact_authority covers class "A" but not class "B"
    When the client calls the remember tool with fact_class "B"
    Then the MCP error code is AUTHZ_CLASS_FORBIDDEN
    And no WorkJob was enqueued
    And an audit record naming fact_class "B" was written by the Service Layer

  Scenario: Happy path — the same authorised class write succeeds on the MCP edge
    Given a write-scoped bearer whose memory_fact_authority covers class "A"
    When the client calls the remember tool with fact_class "A"
    Then the tool result carries a WriteAck with status "accepted"
    And the same two calls through the REST edge produce the same two outcomes
```

#### Performance, Security, Observability

- **Performance targets:** MCP-edge in-process overhead (bearer extraction, correlation id, argument deserialisation, result serialisation — excluding the C9 call) **≤ 5 ms p95**, matching the REST edge; the recall operation's end-to-end ≤ 200 ms p95 (NFR-P2) is met by C6 via C9, unchanged.
- **Security:** every tool call requires a valid OIDC bearer, validated by C3 inside C9; identity is never taken from tool arguments; the same per-tenant isolation, rate limiting, audit **and fact-class write authority** apply because they are C9's (SA-AUTHZ-01, SA-SVC-01). The edge adds no authority check of its own and could not weaken one: `fact_class` is an argument it forwards, not a decision it makes. The token, tool arguments, and fact content are never logged or audited. `MEMORY_ENV=production` suppresses internal error detail in the MCP error `message`. The MCP transport is networked streamable-HTTP only (no stdio / ambient identity), preserving AUTH1–AUTH7 (ADR-016).
- **Observability:** metrics `recall_mcp_tool_calls_total{tool,outcome}`, `recall_mcp_edge_overhead_seconds{tool}` (histogram), reusing C9's `recall_service_calls_total` for the orchestration view. Log fields: `correlation_id`, `subject`, `tenant`, `jti`, `tool`, `outcome`, `code`, `latency_ms` (subject/tenant/jti come back from C9 on the call). Trace spans: `mcp.tools_call` (root, attributes `recall.tool`, `recall.correlation_id`) wrapping the C9 `service.call` span.

#### Gaps

None.

---

#### Amendment January 2026 — Collections, rank fusion and typed sub-queries (ADR-020)

> Source: HLD revision 0.11.0, ADR-020; requested by ordin's ADR-110. Every rule below adds to the sections above. Owners of the behaviour: `retrieval-engine.md` (sub-queries, validation step 1′), `memory-store.md` (collections, fusion), `service-layer.md` (remember's collection, `check_content`), each amendment January 2026.

##### Purpose (addition)

The MCP API Edge carries the new request fields to the Service Layer unchanged: `tools/call` `recall` and `remember` input schemas (generated from the 2C types, so `tools/list` advertises them) gain `searches` and `filters.collections` on recall and `collection` on remember. Every `Fact`-bearing result carries `collection` when it is not the default `""`. `check_content` is not exposed (SA-SVC-CHECK-01); nor is `erase_collection`.

##### Approach (addition)

The edge stays a thin adapter: the schema is generated from the same DTOs, so the REST and MCP shapes cannot diverge, and validation stays in C6 and C9.

##### Shared Context (additions)

```rust
// src/types/api.rs (2C.4), duplicated from retrieval-engine.md, memory-store.md and service-layer.md
pub struct RecallRequest {
    #[serde(default)] pub query: String,                   // exactly one of a non-empty query and searches
    #[serde(default)] pub searches: Option<Vec<SubQuery>>, // 1–4
    #[serde(default)] pub filters: RecallFilters,           // + collections: Option<Vec<String>> (1–32, ^[a-z0-9:_-]{0,64}$)
    #[serde(default = "default_result_cap")] pub result_cap: u8,
    #[serde(default)] pub cursor: Option<String>,
    #[serde(default)] pub include_provenance: bool,
}
pub struct SubQuery { #[serde(rename = "type")] pub kind: SubQueryKind /* "lex" | "vec" */, pub query: String /* 1–4,096 */ }
pub struct RememberRequest { /* … */ #[serde(default)] pub collection: Option<String> }
// Fact (2C.2): #[serde(default, skip_serializing_if = "String::is_empty")] pub collection: String
```

##### Public Interface (changes)

`tools/call` `recall` and `remember` input schemas (generated from the 2C types, so `tools/list` advertises them): `query` is no longer marked required; the schema states "exactly one of `query` and `searches`" in its description and C6 enforces it. Example recall arguments: `{"searches":[{"type":"lex","query":"orders table"},{"type":"vec","query":"who owns order data"}],"filters":{"collections":["tk:b"]},"result_cap":5}`.

##### Internal Logic

N/A — unchanged: deserialisation and validation happen inside the Service Layer (C9) and C6.

##### Data Model

N/A — no persistence.

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Both or neither of `query` and `searches`; more than 4 sub-queries; `filters.collections` empty, over 32, duplicated or malformed | 400 | VAL_OUT_OF_RANGE | `{"error":{"code":"VAL_OUT_OF_RANGE","message":"recall request field out of range","correlation_id":"<uuid>"}}` |
| `remember.collection` malformed | 400 | VAL_INVALID_BODY | `{"error":{"code":"VAL_INVALID_BODY","message":"invalid request body","correlation_id":"<uuid>"}}` |

No new registry code.

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: MCP API Edge — collections and typed sub-queries

  Scenario: Happy path — typed recall through the edge
    Given a valid token with memory.read
    When recall is sent with searches [{"type":"lex","query":"orders table"},{"type":"vec","query":"who owns order data"}]
    Then the Service Layer receives both sub-queries unchanged and the result carries ranked facts

  Scenario: Edge — a fact in a collection shows it
    Given a fact with collection "tk:b" that matches the query
    When recall is sent
    Then that fact's JSON carries "collection":"tk:b", and a fact of the default collection carries no "collection" key

  Scenario: Error — query and searches together
    When recall is sent with query "x" and searches [{"type":"lex","query":"y"}]
    Then the response carries code VAL_OUT_OF_RANGE

  Scenario: Error — check_content is not offered
    When the caller lists the operations
    Then no "check_content" operation is listed
```

##### Performance, Security, Observability (additions)

- **Performance:** edge overhead target unchanged (≤ 5 ms p95); larger request bodies stay within the existing body limit.
- **Security:** collections narrow the caller's read filter and never widen it; sub-query text is never logged.
- **Observability:** none added at the edge; C6 counts typed recalls.

##### Gaps

None.

---

#### Amendment January 2026 — Entities, links and graph recall (ADR-021)

> Source: HLD revision 0.12.0, ADR-021; requested by ordin's ADR-111. Every rule below adds to the sections above and to the January 2026 collections amendment. Owners of the behaviour: `service-layer.md` (validation of the new remember fields, the new operations), `write-pipeline.md` (writing entities and links), `retrieval-engine.md` (graph recall, `fading`, `links`), each amendment January 2026.

##### Purpose (addition)

The MCP API Edge carries the new fields unchanged: the `remember` input schema gains `entities`, `links` and `derived_from`, and every `recall` result carries `fading` on each fact and `links` when recall followed any. No tool is added: the four new read operations are offered on the HTTP edge only, and the six person actions are ordin-in-process only (SA-GRAPH-09).

##### Approach (addition)

The schema is generated from the shared 2C types, so the two edges cannot disagree about the remember shape. Rejected: *MCP tools for the new reads* — an agent recalls through `recall`, which now uses the graph itself; listing entities is a person's view and has no agent caller in this design.

##### Shared Context (additions)

```rust
// src/types/api.rs (2C.4), duplicated from service-layer.md and retrieval-engine.md
pub struct RememberRequest { /* … */
    #[serde(default)] pub entities: Vec<EntityAssertion>,   // 0..=16
    #[serde(default)] pub links: Vec<LinkAssertion>,        // 0..=16
    #[serde(default)] pub derived_from: Vec<String>,        // 0, or 2..=64; only with memory_class = consolidated
}
pub struct EntityAssertion { pub name: String, #[serde(default)] pub aliases: Vec<String> }
pub struct LinkAssertion { pub from: String, pub kind: String, pub to: String,
                           #[serde(default)] pub ended: bool, #[serde(default)] pub confidence: Option<f64> }
pub struct RankedFact { /* … */ pub fading: bool }
pub struct RecallResponse { pub facts: Vec<RankedFact>, #[serde(default, skip_serializing_if = "Vec::is_empty")] pub links: Vec<RecallLink> }
```

##### Public Interface (changes)

`tools/list` advertises the regenerated `remember` schema. `tools/call` `remember` description gains: "Name the things the fact is about in `entities`, and state links between them in `links`; set `ended` for a link that is no longer true." Example arguments: `{"content":{"statement":"Borealis supplies packaging","subject":"Borealis"},"entities":[{"name":"Borealis","aliases":["Borealis Ltd"]},{"name":"packaging"}],"links":[{"from":"Borealis","kind":"supplies","to":"packaging","confidence":0.9}],"idempotency_key":"k-77"}`. The `recall` result's structured content carries `links` and each fact's `fading`.

##### Internal Logic

N/A — unchanged: validation happens in C9.

##### Data Model

N/A — no persistence.

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `entities`, `links` or `derived_from` break a rule, or a link names a thing the request does not | 400 | VAL_INVALID_BODY | `{"error":{"code":"VAL_INVALID_BODY","message":"invalid request body","correlation_id":"<uuid>"}}` |

No new registry code.

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: MCP API Edge — entities and links

  Scenario: Happy path — remember with a link through the edge
    Given a valid token with memory.write
    When remember is sent with entities Borealis and packaging and a link "supplies"
    Then the Service Layer receives the entities and the link unchanged and the result is a WriteAck

  Scenario: Edge — recall shows the link it followed
    Given a fact reached through the link Borealis–supplies–packaging
    When recall is sent with query "who supplies packaging?"
    Then the structured content carries a links entry naming "Borealis", "supplies" and "packaging"

  Scenario: Error — an endpoint the request does not name
    When remember is sent with links [{"from":"Borealis","kind":"supplies","to":"cartons"}] and no entity "cartons"
    Then the response carries code VAL_INVALID_BODY

  Scenario: Edge — no graph tools are listed
    When the caller sends tools/list
    Then the tools are exactly recall, remember, get, retire, delete and capabilities
```

##### Performance, Security, Observability (additions)

- **Performance:** edge overhead target unchanged (≤ 5 ms p95).
- **Security:** names, aliases and kinds are never logged at the edge; the person actions are not reachable over MCP.
- **Observability:** none added at the edge.

##### Gaps

None.
