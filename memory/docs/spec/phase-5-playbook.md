# Phase 5 — Generation Playbook

> **Spec set:** `memory` (agentic memory service) · **Mode:** greenfield
> **derivedFromHld:** 0.12.0 · **Source HLD:** `docs/design/agentic-memory/` · **Authored:** January 2026 · **Amended:** January 2026 (RFC 01, ADR-014; RFC 02, ADR-015), January 2026 (RFC 02-consumer-tenancy-profile, pass 1 — Step 6c added), January 2026 (ordin ADR-110, ADR-020 — Step 6f added), January 2026 (ordin ADR-111, ADR-021 — entities, links and graph recall)

The ordered build checklist `codegen` follows, one Playbook step per invocation. Steps respect the
Phase 2B dependency DAG: every component depends only on lower-numbered steps. The cross-cutting
concerns (X1–X13) are Phase-0 foundations built into scaffolding and consumed everywhere.

**Toolchain:** Rust + Cargo (ADR-009) plus the active **Practice Pack** for exact build/test/lint
commands (HLD 05; `codegen`/`testgen` read them from the Pack — this Playbook names *what* to run, not
the literal command strings). Test strategy is outside-in BDD with testcontainers + Dex for OIDC and
SurrealDB's in-memory backend for the fast inner loop (ADR-010).

---

## Step 0 — OQ-STORE spike (precondition, before any Rust scaffolding)

**Load-bearing for ADR-009.** Validate that embedded SurrealDB meets the latency/scale targets
(NFR-P2 p95 ≤ 200 ms, NFR-P3 ANN ≤ 50 ms) for graph + vector (HNSW) + keyword (BM25) over rich
bi-temporal edges, **and** exercise the remote SurrealDB/TiKV path through the same engine abstraction.

- [ ] Build a throwaway spike: load representative facts, run ANN + BM25 + 2-hop graph queries, measure p95.
- [ ] Exercise both embedded (SurrealKV) and remote modes behind one abstraction.
- [ ] **Gate:** if the spike fails, **stop** — ADR-003 *and* ADR-009 reopen (the Rust/embedded rationale collapses); return to `/design`. Do not scaffold Rust until this passes (HLD 10 risk, OQ-STORE).

## Step 1 — Scaffolding & Phase-0 foundations

- [ ] Cargo project for crate `recall`; module skeleton: `store`, `queue`, `auth`, `write_pipeline`, `retrieval`, `maintenance`, `api`, plus `types/`, `config.rs`, `error.rs`, `obs/`, `providers/`, `shutdown.rs`. (No `freshness` module — C5 retired by ADR-014.)
- [ ] **X1 Error Handling** — `AppError` (§2C.7), the canonical error-code registry, envelopes (§2C.1), `map_error`, panic-recovery layer.
- [ ] **X6 Configuration** — typed `Config` from env > file > default with startup validation (§2D, 40+ keys); fail-fast on missing required keys and the embedding-dimension check (SA-EMBED-01).
- [ ] **X3/X4/X5 Observability** — `tracing` JSON logging with correlation-id + redaction, the metric catalogue, OTLP tracing with async context propagation.
- [ ] Shared types (§2C.1–§2C.6): envelopes, domain entities (incl. `Fact.pii_review`), scope + read-filter, API payloads (incl. `RecallRequest.include_provenance`, `RankedFact.source: Option<SourceProvenance>`), work-queue types, the `MemoryStore`/`WorkQueue` and provider traits, `ProviderError`/`PiiSpan`.
- [ ] Provider adapters (`src/providers/`): thin HTTP impls of `EmbeddingClient`, `RerankClient` with timeouts + bounded retry; test stand-ins. (No `BrokerClient` — ADR-014; no `LlmClient` — ADR-015.) The `PiiDetector` default impl is **in-process deterministic** (regex/pattern, no network call — ADR-015).
- [ ] CI wiring per the Practice Pack: build, test, lint, coverage gate (≥70%), `secscan`.

## Step 2 — C1 Memory Store (Phase 1)

- [ ] Implement the full `MemoryStore` trait (`components/memory-store.md`) over embedded SurrealDB; namespace-per-tenant (ADR-011); HNSW + BM25 + graph signals; bi-temporal CRUD; `supersede`; verifiable `hard_delete` with `DeletionProof`; maintenance scans; `append_audit`; `list_tenants`.
- [ ] **X7 Migrations** — the `Migrator`, `0001_init` up/down per tenant (schemaless fact/entity/relationship/source + schemafull append-only `audit_log`), `schema_migrations`, dry-run.
- [ ] Tests: unit for read-filter + bi-temporal logic; integration (SurrealDB in-memory + container) for every error-table row and Gherkin scenario, incl. cross-tenant isolation and verifiable-delete proof.

## Step 3 — C2 Work Queue · C3 Auth & Scope (Phase 2, independent — may build in parallel)

- [ ] **C2** (`components/work-queue.md`): store-backed `work_job` + `dead_letter`, atomic claim/lease, idempotent enqueue dedup, backoff+jitter→dead-letter, lease-reaper over `list_tenants`. Tests for concurrent-claim safety + reaper.
- [ ] **C3** (`components/auth-scope.md`): OIDC discovery + JWKS cache/refresh, full token validation (alg-allowlist, iss/aud/exp/nbf), `ScopeContext` construction, `authorise`, read-filter helper. Tests use **Dex** (ADR-010): valid/expired/wrong-aud/missing-scope.

## Step 4 — C4 Write Pipeline (Phase 3)

- [ ] **C4** (`components/write-pipeline.md`): the 8-step pipeline (filter→intake(structured, no LLM)→normalise→entity-resolve(rules→ML→create-new)→score→PII scan(in-process)→write gate→embed+persist), `quarantine` table, idempotent persist. Tests for admit/quarantine/reject, PII redaction/flag, replay idempotency, non-object-content rejection.
- [ ] **C5 Freshness Checker — RETIRED (ADR-014):** not built. Freshness is agent-side; `memory` performs no source-change check and makes no outbound broker call.

## Step 5 — C6 Retrieval Engine · C7 Maintenance Worker (Phase 4)

- [ ] **C6** (`components/retrieval-engine.md`): the read pipeline (embed→stage-1 multi-signal→rerank→recency→gate/abstain→cursor→provenance-attach (conditional, ADR-014)), SA-LAT-01 sub-budgets + degradation. Tests for happy/abstain/rerank-timeout-degrade/store-timeout/provenance-on-off + the p95 budget assertion.
- [ ] **C7** (`components/maintenance-worker.md`): scheduler (idle via activity probe + fallback timer) + queue-consumer (claims `ReEmbedFact`/`HardDelete`); four duties — supersession, decay (Ebbinghaus + salience floor), re-embed, verifiable hard delete; `maintenance_state`. No consolidation (agent-side, ADR-015). Unit-test the decay pure core against case tables.

## Step 6 — C9 Service Layer + C8 HTTP API Edge (Phase 5/6; ADR-016)

- [ ] **C9 Service Layer** (`components/service-layer.md`): extract the transport-agnostic orchestration into `src/service` — per-operation methods (recall/remember/get_fact/retire/delete/capabilities) running the fixed chain **X2** auth(C3) → authorise → **X9** rate limit → idempotency (SA-IDEM-01) → component → audit-emit (SA-AUDIT-01) → **X1** error-classification, on a verified `ScopeContext`, returning `CallResult`/`AppError`. Single definition of each concern (SA-SVC-01).
- [ ] **C8 HTTP API Edge** (`components/http-api-edge.md`): refactor `src/api` to a **thin adapter over C9** — axum router for all HLD-08 routes; retains correlation-id → **X12** body-size → bearer-extract → C9 call → render (`Success`/`ErrorEnvelope`, `RateLimit-*`, ETag/conditional-GET, status mapping). External REST contract unchanged.
- [ ] **X8 Health** (`/healthz`, `/readyz`), **X10 Pagination** (opaque cursor), **X11 CORS** (off by default), generated `GET /openapi.json` (SA-VER-01), `GET /metrics`.
- [ ] **X13 Graceful Shutdown** — signal handling, drain, worker lease release, store flush.
- [ ] **Regression gate:** the existing BDD suite (`cargo test --test bdd`) passes **unmodified** — proof the C8 refactor is behaviour-preserving.
- [ ] Tests: C9 orchestration unit tests with no transport types in scope; per-route happy/edge/error; idempotent replay; rate-limit headers; audit emission; auth/authz matrix.

## Step 6b — C10 MCP API Edge (Phase 6; ADR-016)

- [ ] **C10 MCP API Edge** (`components/mcp-api-edge.md`): new binary `memory-mcp` (`src/mcp`); add the `[[bin]]` target and a maintained Rust MCP library (OQ-LIB) to `Cargo.toml`; build the same `build_state`, construct `Service` (C9), serve MCP over streamable-HTTP at `MEMORY_MCP_HTTP_ADDR`+`MEMORY_MCP_PATH`.
- [ ] One tool per operation (recall/remember/get/retire/delete/capabilities) + `tools/list`; input schemas generated from the same 2C types that back the OpenAPI document; bearer from the HTTP `Authorization` header; `AppError`→MCP-error using the same registry `code` as REST (SA-MCP-MAP-01).
- [ ] Tests: tool happy/edge/error; `tools/list` schema parity; idempotent replay over MCP; **error-code parity with the REST edge** (same input → same `code`); identity-enforced (no bearer → `AUTH_MISSING_TOKEN`, no audit).

## Step 6c — C11 Tenancy Benchmark Harness (Phase 7; RFC 02-consumer-tenancy-profile pass 1, legs re-pointed by RFC 03)

Measurement, not feature work. This step answers whether the consumer tenancy profile is viable before
any of the RFC's engineering is specified or built. Nothing downstream depends on it, so it may be
built at any time after Step 2 (C1); it is placed here because its legs reuse the retrieval path from
Step 5.

**The legs below are not the ones this step first specified.** As first written it provisioned
populations of 10,000 and 50,000 namespaces and measured a per-namespace footprint, a tenant-sweep
cost and a single-tenant latency figure. RFC 03 moves away from that topology and calls the sweep
problem absent at ten namespaces, so those legs are **withdrawn** — `provision`, `footprint`, `sweep`
and the single-tenant latency leg exist in no code, and the two acceptance criteria that decide the
design are instead relevance across co-resident users and absolute stage-1 latency across visible
fractions. The safety gates, the report shape and the ceiling handling carry over unchanged; the
populations and the legs change.

- [ ] **C1 prerequisite:** widen `Migrator::render_up` from private to `pub(crate)` (`src/store/migrate.rs`, SA-BENCH-RENDER-01). No other C1 change; C1's public surface is unchanged.
- [ ] **C1 measurement seam:** a `Store` constructor that opens an arbitrary endpoint at an explicit profile, and a `pub(crate)` vector-leg seam (`recall_vector_only`) that runs the same query the production read path runs, under a caller-chosen `VectorStrategy`. A benchmark that re-implements the query measures its own re-implementation (RISK-001); the seam's standing tripwire asserts it selects the same facts, in the same order, as the public `recall` path under both strategies.
- [ ] **C11** (`components/tenancy-benchmark.md`): new binary `memory-bench` (`src/bench`); add the `[[bin]]` target to `Cargo.toml`. `BenchHarness::{connect, run, teardown}`, `preflight` and `generate_ids`; the `MEMORY_BENCH_*` config keys (§2D); the `BenchReport` JSON artefact.
- [ ] **Safety gates first** (these are the reason the harness is safe to run at all, so they are built and tested before any code that can create or drop a namespace): `MEMORY_BENCH_STORE_URL` required and never inherited from `MEMORY_STORE_PATH`/`MEMORY_STORE_REMOTE_URL` (SA-BENCH-ISOLATION-01); the pre-flight prefix guard that refuses to run when any existing namespace matches `MEMORY_BENCH_TENANT_PREFIX` (SA-BENCH-ISOLATION-02); teardown driven by the generated id list rather than `list_tenants()`, with a prefix check immediately before each drop.
- [ ] **The rig and the corpus.** An embedded **SurrealKV store at a real path** — not a container and not `mem://`: NFR-P3 is a claim about the in-process embedded engine, and both a network hop and an in-memory engine measure something else. One deterministic corpus builder shared by every leg, with the caller's facts spread through the index range rather than taken from the front (taken from the front they are the nearest neighbours, and every fill ratio reads 1.0 whatever the visible fraction). One rig directory per sweep point, opened in sequence: SurrealKV holds an exclusive directory lock for the process lifetime, and a corpus rebuilt in place measures how many rewrites the index has absorbed rather than the variable being swept.
- [ ] **Relevance leg — RFC 03 acceptance criterion 3.** Both strategies over a fixed query set at each co-resident user count, against one store holding one corpus, graded against the exact arm as baseline. Starvation is a **fill ratio** — `returned / min(stage1_k, visible)`, a ratio to what was available — because the failure mode is the post-filter discarding the caller's rows after the index walk, which collapses the ratio while the raw returned count stays plausible.
- [ ] **Latency leg — RFC 03 acceptance criterion 4.** Both strategies at each visible fraction with the visible set pinned at one consumer's memory, so an absolute claim about a 1,000-fact filtered set is measured at 1,000 facts. The crossover is recorded as the **interval** between the bracketing grid points together with the grid itself, never as a point estimate the grid cannot support, and a run that never crosses records null bounds with the grid intact.
- [ ] **Co-residency leg.** The visible set pinned while the namespace grows underneath it, sampled by namespace size so the sweep reaches sizes no fraction grid asks for.
- [ ] **Every row carries its rig and its spread.** `engine`, `engine_version` and host disk on the report; `min_max_ratio` beside the percentiles on every row; a fill ratio on every leg's rows. The defect that forced the re-pointing was a 22x spread between two runs of one nominal configuration that a median-only report did not show, and the post-filtered arm gets *faster* as the visible fraction falls because it finds and returns almost nothing — a timing quoted without its fill ratio reads exactly backwards.
- [ ] **No verdict fields.** The report records measures and the thresholds they are read against (`nfr_p3_millis`), never whether a figure is acceptable. That is a user decision, and a boolean would hide it behind a number.
- [ ] **Ceiling handling — RFC 02 open question 1.** Verify by test that an engine refusal mid-population is recorded as `ceiling: { failed_at_index, engine_error }`, the report is still written, and the process exits **0**. A ceiling is an answer, not a failed run.
- [ ] **Documentation.** Publish the measured figures where a deployment engineer will find them: a sizing runbook under `docs/runbooks/`, not only in `bench-report.json`. Every figure is labelled with its engine and its rig — RFC 03 open question 4c may move the embedded backend from SurrealKV to RocksDB, and a figure quoted without its engine outlives the engine it was true of.
- [ ] Tests: the Gherkin scenarios in `components/tenancy-benchmark.md`. The two error-path scenarios (unset store URL refuses to run; teardown never drops a namespace the run did not create) are the ones that matter most — they are what stands between a mistyped invocation and tens of thousands of dropped namespaces — and both run against a small population in CI.
- [ ] **Exit gate — the D2-D4 decision.** The numbers are the input to RFC 03's blocking decision about whether Stage 2 proceeds on this design. Feed the report back to `/spec`, and update OQ-VOLUMES in the HLD with the pinned figures. If filter-first does not hold NFR-P3 at 1,000 visible facts on the real engine, D2 is refuted and Stage 2 does not proceed on this design — which is the outcome this instrument exists to make possible.

**Withdrawn from this step by RFC 03**, recorded so a reader does not reconstruct them from the RFC's
criterion numbers: the 10,000/50,000 populations, the per-namespace footprint triple
(SA-FOOTPRINT-01), the three-part sweep decomposition (SA-SWEEP-COST-01), the
`MEMORY_BENCH_ISOLATION=database` comparison (OQ-NS-VS-DB / FU-006, moot under RFC 03's D3), the
single-tenant latency leg, and the remote leg. `IsolationShape::Database` stays declared — it is
configuration, not a leg — but nothing measures it until FU-006 is reopened.

## Step 6d — Write authority by fact class (RFC 04, ADR-019)

Touches C3, C9, C8, C10 and C1's audit row. It is a **cross-cutting amendment**, not a new component,
so it has no place in the DAG of its own — it lands after Step 6b, once both edges exist, because
acceptance criterion 6 is asserted against both surfaces and cannot be asserted against one.

- [ ] **C1 — migration `0005_audit_authority`.** Two additive `DEFINE FIELD IF NOT EXISTS … TYPE
      option<string>` on `audit_log` (`client`, `fact_class`); `AuditEntry` grows the matching
      `Option<String>` fields with `#[serde(default)]` so rows written before the migration
      deserialise. Dry-run the migration before applying (SA-MIG-01). Reversing it discards the two
      columns — see the C1 rollback note.
- [ ] **C3 — claim extraction.** `ScopeContext` gains `client: Option<String>` and
      `fact_authority: Option<Vec<FactAuthority>>`; `AuthConfig` gains `client_claim` and
      `authority_claim`; `validate` gains the strict parser (SA-AUTHZ-03) and the start-up log line.
      An absent authority claim is `None`, an empty array is `Some(vec![])`, and the two mean
      different things.
- [ ] **C9 — the enforcement point.** The class check goes in the `remember` chain, after the
      idempotency replay and before the enqueue, on the `finish_error` path so the refusal is audited
      (SA-AUTHZ-01, SA-AUTHZ-04). `AppError::ClassForbidden` → `AUTHZ_CLASS_FORBIDDEN` (403). The
      audit entry carries `client` and the `fact_class` label. Source binding ships in the same check
      (SA-AUTHZ-05).
- [ ] **C8 / C10 — the label only.** `RememberRequest` gains `fact_class: Option<String>`; both edges
      forward it and neither judges it. The MCP tool's `inputSchema` picks the field up from the same
      DTO the OpenAPI document is built from, so the two surfaces cannot disagree about its shape.
- [ ] **Tests.** The existing write suite must pass **unmodified** — that is the proof of acceptance
      criterion 5, and modifying it to accommodate the change would destroy the evidence. Add: the
      class-refusal audit row read back from the store (not from a log); the source-binding pair; the
      unparseable-claim 401; the absent-claim admission plus its counter; the rate-budget and
      idempotency-key consequences; the retire/delete non-interaction; and the same authorised and
      unauthorised write asserted **through both edges**.
- [ ] **Gate.** The trust gate's three SA-WGATE-01 outcomes and the imperative-pattern cap are
      asserted by their existing tests, passing untouched (ADR-008 is not reopened).

## Step 6e — Storing the asserted fact class (RFC 05, SA-AUTHZ-09/-10)

Completes Step 6d: RFC 04 authorises a class and then discards it, so the label the gate judged and
the label a consumer acts on are different strings with nothing keeping them in step. Touches C9, C4,
C1 and the shared `Fact` type. Lands after 6d, which it depends on entirely.

- [ ] **C9 — carry it.** `remember_payload` adds `fact_class` to the enqueued job payload. The value
      has already been authorised by step 5a; this step judges nothing.
- [ ] **C1 — store it on the fact.** `Fact` gains `fact_class: Option<String>` with
      `#[serde(default)]` — without it, `row_to_fact` fails to decode every row written before the
      field. `fact_to_object` writes one more key. **No migration:** `fact` is SCHEMALESS.
- [ ] **C4 — store it on a held write.** `JobPayload` and `QuarantineRecord` gain the field; migration
      `0006_quarantine_fact_class` declares it on the SCHEMAFULL `quarantine` table, which would
      otherwise reject the whole quarantine write. On a superseding write the successor carries the
      class of *that* write, never inherited (SA-AUTHZ-10).
- [ ] **Read path — nothing to do, and prove it.** `RankedFact` wraps `Fact`, so the field reaches the
      recall response and the fact read with no edge change. Assert that rather than assume it.
- [ ] **Tests.** A classed write stores and returns its class, read back from the store rather than
      from the write ack; an unclassed write omits the key rather than returning null or empty; a row
      written by the previous build still decodes; a quarantined write keeps its class; the same two
      writes carry the same two classes through **both** edges; `RecallFilters` is unchanged.
- [ ] **Gate.** The existing write and retrieval suites pass unchanged, and RFC 04's write-authority
      tests keep passing untouched — 6e adds storage, it does not revisit the decision.

## Step 6f — Collections, rank fusion and typed sub-queries (ordin ADR-110, ADR-020)

Touches C1, C6, C9, C8, C10, C11 and the shared types. Lands after 6e. ordin consumes it as change
M-8 (`docs/spec/ordin-desktop/components/MemoryService.md`, amendment January 2026), whose SQL storage
applies the same rules (SA-COLL-05).

- [ ] **Types.** `collection: String` (`#[serde(default)]`, omitted when `""`) on `Fact`, `Entity`,
      `Relationship`, `Source`; `RecallFilters.collections`; `RecallRequest.searches`, `SubQuery`,
      `SubQueryKind`; `RememberRequest.collection`; `CheckContentRequest`, `ContentCheck`,
      `PersonalDataFinding`; C1's `StageOneQuery.searches`, `StageOneSearch`, `Candidate.fused_score`,
      `Candidate.fused_norm`; trait `CollectionErasure`.
- [ ] **C1 — migration `0007_fact_collection`** (field, backfill to `''`, index on four tables;
      `LATEST = 7`); `put_*` validate the label; steps 9′ (runs per list and collection, filter-first)
      and 10′ (fusion, quota, cap) replace the `BTreeMap` merge at `src/store/mod.rs:716`;
      `erase_collection` (step 20c).
- [ ] **C6 — steps 1′, 3′, 4′, 5′:** validation, one embed call for every `vec` sub-query, weights,
      rerank query, `fused_norm` fallbacks.
- [ ] **C9 — `remember.collection`; `check_content`** (ordin-in-process only, audited without text).
- [ ] **C8 / C10 — schemas** regenerated from the DTOs; `check_content` and `erase_collection` not routed.
- [ ] **C4 — write** the request's collection onto the fact, any entity or source it creates, and a
      quarantine record (C4's portion of `0007`: `quarantine.collection`).
- [ ] **C11 — collection leg** (fairness, exact term, filtered, stage-1 timing, fixture) with the
      as-built merge reproduced as the baseline.
- [ ] **Tests.** Every Gherkin scenario of the five amendments; a row written before `0007` decodes
      with collection `""`; one collection with a union ≤ `stage1_k` returns the as-built candidate set.
- [ ] **Gate.** The retrieval regression suite passes unchanged for plain queries whose stage-1 union is
      at most `MEMORY_STAGE1_K`; RFC 04/05 write-authority tests pass untouched; the C11 collection leg
      report is written and its numbers are recorded for ordin's ADR-110 acceptance runs (fairness, exact match, filtered recall and budget).

## Step 6g — Entities, links and graph recall (ordin ADR-111, ADR-021)

Touches C1, C4, C6, C7, C9, C8, C10, C11 and the shared types. Lands after 6f. ordin consumes it as
change M-9 (`docs/spec/ordin-desktop/components/MemoryService.md`, amendment January 2026), whose SQL
storage applies the same rules and owns the SQL DDL (SA-COLL-05).

- [ ] **Types.** `RememberRequest.entities`, `links`, `derived_from`; `EntityAssertion`,
      `LinkAssertion`; `Relationship.replaces`; `RankedFact.fading`; `RecallResponse.links`,
      `RecallLink`; `DeletionProof.relationships_removed`; the request/result types of the ten new
      operations; C1's `StageOneQuery.graph_*` / `phrase_texts`, `Candidate.source_trust` /
      `graph_hop` / `via`; trait `GraphStore`, `LinkRole`, `Neighbour`, `EntitySummary`, `EntityPage`,
      `EntityDetail`, `EntityCount`, `EntityErasureProof`; `memory::text::normalise_name` (moved from C4).
- [ ] **C1 — migration `0008_graph`** (`relationship.replaces`, `relationship_live`, table
      `relationship_fact`, `entity.names_norm` + `entity_names`, `maintenance_state.names_backfilled_at`;
      `LATEST = 8`); `GraphStore` (steps 20d–20k, `neighbours`, `match_entities`); 7′ (`names_norm`),
      9″ (graph and link lists), 11′ / 12′ (closing rule), 13′ (orphaned links in `hard_delete`),
      19d′ (merge folds names and duplicate links).
- [ ] **C4 — steps 2, 4′, 6′, 7′, 8′:** entity assertions and aliases; names and kinds through the PII
      scan and instruction check; links written or ended on the admit path; `derived_from` re-checked;
      C4's portion of `0008` (`quarantine.entities_asserted`, `links_asserted`, `derived_from`).
- [ ] **C6 — steps 4″, 6′, 10″:** graph settings into stage-1, the trust factor, `fading` and `links`.
- [ ] **C7 — `is_fading`, `fading_since`, `DecayConfig`;** backfill in `run_cycle`; no consolidation duty.
- [ ] **C9 — remember validation; the four reads; the six person actions** (ordin-in-process only),
      each audited under its own name.
- [ ] **C8 — four read routes** and regenerated bodies; **C10 — regenerated `remember` schema**, no new tool.
- [ ] **C11 — graph leg** (both backends, hops 1 / 2 / off, the supplies and hop-limit fixtures).
- [ ] **Tests.** Every Gherkin scenario of the eight amendments; every C1 scenario also against the
      SQL storage on SQLite and PostgreSQL (ordin ADR-111 criterion 17); a store migrated from `0007`
      with facts and entities decodes, backfills and recalls (criterion 19).
- [ ] **Gate.** The retrieval regression suite passes unchanged for a store with no relationships and
      no query-matched entity; the collection leg's figures do not regress; the C11 graph leg report is
      written and its numbers recorded for ordin's ADR-111 acceptance runs (criteria 3, 4, 5).

## Step 7 — Integration & verification (whole-system gate)

- [ ] **Outside-in BDD suite** (ADR-010): drive the public API end-to-end through Dex + containerised SurrealDB + queue + provider stand-ins. Map each component's Gherkin to a scenario.
- [ ] Cross-component flows: auth→scope→retrieval; async write→store→retrieval (eventual consistency); supersession over time; recall provenance branch (include_provenance on/off); the agent-side freshness loop (recall returns provenance → agent writes a fresh superseding note → next recall returns it); verifiable delete proof; cross-tenant isolation (NFR-PR1) as a first-class case.
- [ ] **Unit cores**: decay maths, ranking fusion + recency, entity-resolution rules, write-gate thresholds — table-driven.
- [ ] **Gates (blocking):** build clean; lint clean; **coverage ≥ 70%** (C3 rule); `secscan` clean (no hardcoded secrets, parameterised queries only, no PII/token logging); migration dry-run clean.
- [ ] **Smoke**: single-binary boot with embedded store, `/readyz` green, a remember→(async)→recall round-trip, a forget→deletion-proof round-trip.

---

## Build-order summary (DAG)

```
Step 0  OQ-STORE spike  ── gate ADR-009 ──┐
Step 1  Scaffolding + X1,X3,X4,X5,X6      │  (Phase 0 foundations + shared types + providers)
Step 2  C1 Memory Store (+X7)             │  Phase 1
Step 3  C2 Work Queue · C3 Auth & Scope   │  Phase 2  (parallel)
Step 4  C4 Write Pipeline                 │  Phase 3  (C5 retired, ADR-014)
Step 5  C6 Retrieval · C7 Maintenance     │  Phase 4  (parallel)
Step 6  C9 Service Layer → C8 HTTP Edge    │  Phase 5/6  (+X2,X8–X13; C8 thin adapter over C9, ADR-016)
Step 6b C10 MCP API Edge (bin memory-mcp)  │  Phase 6    (second edge over C9, ADR-016)
Step 6c C11 Tenancy Benchmark (bin memory-bench) │ Phase 7 (leaf; measurement only — RFC 02-consumer-tenancy-profile pass 1)
Step 6d Write authority by fact class      │  cross-cutting amendment over C1/C3/C9/C8/C10 (RFC 04, ADR-019)
Step 6e Storing the asserted fact class    │  completes 6d over C9/C4/C1 + shared Fact (RFC 05)
Step 6f Collections + rank fusion         │  over C1/C4/C6/C9/C8/C10/C11 + shared types (ordin ADR-110, ADR-020)
Step 6g Entities, links, graph recall    │  over C1/C4/C6/C7/C9/C8/C10/C11 + shared types (ordin ADR-111, ADR-021)
Step 7  Integration & verification        ┘  whole-system gate
```

> **ADR-016 build note.** C9 must land before C8's refactor and before C10. C8's refactor is gated by
> the existing BDD suite passing unmodified (behaviour-preserving). C10 depends only on C9 and can be
> built in parallel with C8's refactor once C9 exists.

> **RFC 02-consumer-tenancy-profile build note (pass 1).** C11 is a leaf: it depends on C1 and nothing
> depends on it, so it changes no build order and neither shipped binary grows (NFR-MA3 holds). Its one
> prerequisite is the `render_up` visibility widening in C1. Its remote leg is the only part with an
> external dependency — FU-019 (`docs/wip/plan/04-remote-store-signin.md`) for a *secured* remote
> endpoint. The engineering half of the RFC (criteria 4–6, 10) is deliberately absent from this
> Playbook: it is specified only after the benchmark reports.

> **RFC 04 build note.** Step 6d adds no component and no binary. It is sequenced after Step 6b
> because it must be proven on both edges at once (acceptance criterion 6); building it against C8
> alone would satisfy the tests and leave the MCP edge open, which is the failure mode `SA-SVC-01`
> exists to prevent. Its only schema change is additive.

No cycles: each step depends only on lower-numbered steps. The async write path means C8 never calls
C4 synchronously — it enqueues on C2, so there is no C8→C4 edge.
