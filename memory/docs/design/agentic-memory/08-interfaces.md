# 08 — External Interfaces

> **Mode:** draft · **Revision:** 0.12.0 · **Last updated:** January 2026

Names and shapes only. Exact JSON schemas, field types, error codes, and status-code tables belong in
`/spec`. All endpoints are **new**. All require an OIDC bearer token except the unauthenticated
operational endpoints noted.

## HTTP API (versioned under `/v1`)

| Operation | Endpoint (shape) | Purpose |
|---|---|---|
| Discover capabilities | `GET /v1` | Returns the available operations and a link to the OpenAPI document, so an agent can use the API without external docs. |
| Recall | `POST /v1/recall` | Hybrid retrieval; body carries a natural-language query, optional filters, a result cap, and a provenance opt-in. Returns ranked facts with confidence, or an abstention; when provenance is requested, each sourced fact also carries its `origin_ref` + `modification_marker` so the agent can check source freshness itself (ADR-014). |
| Remember | `POST /v1/memories` | Submit content to remember; honours an `Idempotency-Key` header. Returns an idempotent acknowledgement (write is async). |
| Get / freshness-check a fact | `GET /v1/memories/{id}` | Fetch a fact; supports `If-Modified-Since` / `ETag` for cheap currency checks. |
| Retire (forget, non-destructive) | `POST /v1/memories/{id}/retire` | Ends a fact's validity; honours `Idempotency-Key`. |
| Delete (verifiable) | `DELETE /v1/memories/{id}` | Hard deletion including derived summaries and embeddings; explicit intent; returns deletion proof. |
| OpenAPI contract | `GET /openapi.json` | Machine-readable contract for runtime code generation. |

## Operational endpoints (unauthenticated, no fact data)

| Operation | Endpoint (shape) | Purpose |
|---|---|---|
| Liveness | `GET /healthz` | Process is up. |
| Readiness | `GET /readyz` | Store and OIDC discovery reachable. |
| Metrics | `GET /metrics` | Observability scrape endpoint (no fact content). |

## MCP interface (second edge — ADR-016)

The same operations are also exposed over **MCP (Model Context Protocol)** from a separate binary
(`memory-mcp`), as a second manifestation of the one service. Names and shapes only; detail in `/spec`.

| MCP surface | Shape | Maps to |
|---|---|---|
| Tool discovery | `tools/list` | The MCP analogue of `GET /v1` + `/openapi.json` — advertises the tools below with machine-readable input schemas generated from the same types that back the OpenAPI document. |
| `recall` tool | `tools/call` name `recall` | `POST /v1/recall` — hybrid retrieval; same request fields incl. `include_provenance`. |
| `remember` tool | `tools/call` name `remember` | `POST /v1/memories` — async-accepted; the tool result carries the same idempotent acknowledgement. |
| `get` tool | `tools/call` name `get` | `GET /v1/memories/{id}`. |
| `retire` tool | `tools/call` name `retire` | `POST /v1/memories/{id}/retire`. |
| `delete` tool | `tools/call` name `delete` | `DELETE /v1/memories/{id}` — verifiable hard delete, proof returned (unchanged). |

- **Transport:** MCP over **streamable-HTTP** (networked), not stdio.
- **Auth:** the same `Authorization: Bearer <OIDC JWT>` the broker injects for REST; the Service Layer
  derives the `ScopeContext` identically (ADR-016). A call without a valid bearer is rejected with the
  same error class as the equivalent unauthenticated REST call.
- **Errors:** the one error-code registry is rendered as MCP tool/protocol errors (the same `code`s the
  REST edge maps to HTTP statuses).

## Collections and typed sub-queries (ADR-020, revision 0.11.0)

Names and shapes only; detail in `/spec` (`memory-store.md`, `retrieval-engine.md`, `service-layer.md`, amendments January 2026).

| Surface | Shape | Notes |
|---|---|---|
| Recall body (REST and MCP) | gains `searches: [{type: "lex" \| "vec", query}]` (1–4) as the alternative to `query`, and `filters.collections: [label]` (1–32) | Exactly one of `query` and `searches`. Collections narrow the caller's readable facts before every signal. |
| Remember body (REST and MCP) | gains `collection: label` | Carried onto the fact and any entity or source the write creates. |
| `Fact`, entity, relationship and source | carry `collection` (omitted from JSON when `""`) | Returned on every `Fact`-bearing result. |
| Content check | Service operation `check_content(text) → {personal_data: none \| review \| redact, instruction_like}` | In-process ordin caller only; no REST route, no MCP tool. |
| Collection erasure | Store operation `erase_collection(tenant, team, collection) → erasure proof` | In-process ordin caller only; removes one collection's facts, entities, relationships and sources by predicate, keeping any a fact of another collection still names. |

## Entities, links and graph recall (ADR-021, revision 0.12.0)

Names and shapes only; detail in `/spec` (`memory-store.md`, `write-pipeline.md`, `retrieval-engine.md`, `service-layer.md`, `http-api-edge.md`, amendments January 2026).

| Surface | Shape | Notes |
|---|---|---|
| Remember body (REST and MCP) | gains `entities: [{name, aliases}]` (0–16), `links: [{from, kind, to, ended, confidence}]` (0–16), `derived_from: [fact id]` (0 or 2–64, consolidated only) | Link ends must be names of the same request. |
| Recall result (REST and MCP) | each fact gains `fading`; the result gains `links: [{relationship, from_name, to_name, reached}]` (≤ 20) | Only links that led to a returned fact. |
| `GET /v1/memories/{id}/history`, `GET /v1/memories/{id}/origins` | a fact's versions with their links; a summary's origin facts | Read class, read-filtered. |
| `GET /v1/entities?search=&cursor=`, `GET /v1/entities/{id}` | a page of entities with fact and link counts; one entity with its facts and links | Read class, read-filtered. |
| Person operations | `keep_fact`, `count_entity`, `forget_entity`, `merge_entities`, `correct_link`, `forget_link` | In-process ordin caller only; no REST route, no MCP tool. |

## Outbound interfaces

`memory` makes **no outbound calls to the broker or source systems** (ADR-014). Freshness checking and
source re-reads are the agent's responsibility, performed by the agent and its co-located broker; `recall`
stores the version marker the agent supplied at write time and returns it on request (see the Recall row
above and [01 — Context](./01-context.md)). The only outbound calls `memory` makes are to its configured
model providers (embedding, reranker on the read path; LLM off it) — model inferences, not orchestration.

## Conventions (shape-level, detail in `/spec`)

- **Auth header:** `Authorization: Bearer <OIDC JWT>`, injected by the broker.
- **Idempotency:** `Idempotency-Key` header on all writes (remember / retire / delete).
- **Rate limiting:** `RateLimit-*` and `Retry-After` response headers.
- **Envelopes:** one consistent success envelope and one error envelope (`code` + `message`) across
  all endpoints — exact fields specified in `/spec`.
- **Versioning:** path-versioned (`/v1`); breaking changes take a new version.
