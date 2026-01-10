# 10 — Risks, Alternatives, Dependencies, Rollout

> **Mode:** draft · **Revision:** 0.12.0 · **Last updated:** January 2026

## Risks

- **Store immaturity — load-bearing for the language choice.** SurrealDB (or any "does-everything"
  engine) may under-deliver on one capability at the target scale. Because **Rust was chosen primarily
  to embed SurrealDB in-process** (ADR-009), a spike failure is not just a store swap — it **reopens
  the language decision** and the single-binary deployment story. *Mitigation:* run the OQ-STORE spike
  **before Rust scaffolding**, not merely before codegen; keep store access abstracted (NFR-MA1) with a
  documented fallback (graph DB + vector index); accept that the fallback is a rearchitecture.
- **Per-operation namespace preamble scales with tenant population, on the read path.** Selecting a
  tenant namespace is not free: every store operation runs an idempotent provisioning preamble
  (`DEFINE NAMESPACE IF NOT EXISTS`, `USE NS`, `DEFINE DATABASE IF NOT EXISTS`, the migration
  bookkeeping DDL, and a `schema_migrations` read) before it does its own work, and it does so on the
  read path as well as the maintenance path. At tens of organisations this is invisible; at a consumer
  tenant population (10,000–50,000 single-user namespaces) it is a constant factor paid per request,
  so **NFR-P2 becomes exposed to tenant count and not only to corpus size** — a coupling the design
  did not previously acknowledge. The tenant sweep compounds it: the scheduler and the queue reaper
  each enumerate every namespace per tick and pay the preamble per tenant. *Mitigation:* the Tenancy
  Benchmark Harness times the three parts — namespace enumeration, the per-tenant due-probe, and the
  preamble — separately rather than reporting one tick figure, so that remediation cannot fix the
  sweep and leave the read-path cost in place. Whether remediation is warranted is decided by the
  measurement, not in advance.
- **Predicate isolation is weaker than the structural isolation the design was written around.**
  Where a deployment puts several tenants in one namespace, nothing in the engine stops a query from
  reaching a co-resident tenant's rows — the separation is carried by `owner.tenant` on every read
  filter and on the erasure predicate (ADR-018), so a filter written without it, or an erasure written
  against the namespace rather than the predicate, is a cross-tenant disclosure or a cross-tenant
  destruction rather than a bug in one tenant's results. Two instances of exactly this have already
  been found and closed. *Mitigation:* keep the tenant conjunct on every filter arm and every
  destructive path; test co-residency directly by writing a foreign tenant's rows into a namespace,
  which no public API can produce and which therefore never happens by accident in a suite that uses
  one tenant per namespace; and state per deployment which of the two isolation forms it has, rather
  than claiming the stronger one everywhere.
- **Embedded↔remote store abstraction.** The claim that scaling from embedded in-process to a remote
  SurrealDB / TiKV cluster needs "no rewrite" (ADR-009/011) spans different transaction, latency, and
  error semantics. *Mitigation:* treat the engine abstraction (NFR-MA1) as a first-class, tested seam;
  exercise the remote path in the spike, not only the embedded one.
- **Benchmark-vs-reality gap.** Memory that scores well on QA benchmarks can crater inside whole
  agentic tasks. *Mitigation:* evaluate end-to-end on representative agentic-search workflows with
  evolving facts, not just LoCoMo/LongMemEval (`good-mem.md` §13).
- **Memory poisoning.** Booby-trapped source content tries to write false facts or inject
  instructions. *Mitigation:* write-gate trust validation (ADR-008), trust-aware retrieval,
  untrusted-data separation, provenance on every fact.
- **Self-reinforcing error.** A wrong consolidated insight becomes permanent "knowledge".
  *Mitigation:* validate insights against sources before promotion, decaying confidence on
  inferences, never let an insight outrank its source facts (ADR-006, sequences §Consolidate).
- **Confident staleness.** A high-relevance fact returned after its source changed. *Mitigation:*
  freshness loop on the read path, recency weighting, conditional requests.
- **Trust-threshold miscalibration.** The write gate rejects good facts or admits poison.
  *Mitigation:* tune against labelled data; quarantine (not hard-reject) the uncertain; monitor
  rejected-write metrics.
- **Eventual-consistency surprise.** A just-written fact is not immediately retrievable (async write
  path). *Mitigation:* document the contract; idempotent acks; fast-track indexing where needed.
- **Entity-resolution precision in v1.** v1 resolves entities by deterministic rules then ML
  similarity, and *creates a new entity* rather than risk a wrong merge when neither tier is confident;
  LLM-based adjudication is deferred. *Mitigation:* a maintenance-time `merge_entities` pass folds
  duplicates; revisit LLM adjudication once duplication rates are measured (surfaced by `/spec` Phase
  3.5 from the Write Pipeline LLD).
- **Cross-scope leakage.** A bug surfaces one user's fact to another. *Mitigation:* scope enforced
  server-side on the authenticated claim; isolation tested as a first-class case (NFR-PR1).
- **Slow / flaky integration tests.** Containerised dependencies make suites heavy. *Mitigation:*
  reuse/singleton container sessions, in-memory SurrealDB for the inner loop, parallel scope-isolated
  scenarios, Dex-in-a-container for hermetic OIDC (ADR-010).

- **Stage-1 cap kept the oldest candidates, not the best (closed by ADR-020, revision 0.11.0).** When
  the vector, keyword and graph signals together surfaced more than `stage1_k` facts, the merge kept the
  first `stage1_k` in fact-id order (`src/store/mod.rs:716`); fact ids are UUIDv7, so the newest relevant
  facts were the ones dropped before rerank. *Mitigation:* rank fusion with a per-collection quota caps by
  fused rank (memory-store amendment January 2026, step 10′); the benchmark's collection leg records the
  as-built merge as its baseline.
- **Per-collection stage-1 cost.** Runs per recall are lists × collections (up to 4 sub-queries, plus the
  graph list, over up to 32 collections), and a collection run under a collection filter uses the
  filter-first exact vector scan, O(visible facts of the collection). A deployment with many large
  collections can exceed the 50 ms stage-1 sub-budget (NFR-P3). *Mitigation:* targets stated for ≤ 32
  collections; the collection leg measures 3 × 20,000 facts with 4 sub-queries; a single-collection,
  unfiltered recall keeps the profile's strategy unchanged.
- **Decay never prunes, and its time unit is seconds (as-built; recorded by ADR-021, revision 0.12.0).**
  `retrievability` divides elapsed seconds by `s·k` with `MEMORY_DECAY_K = 10`, a 10-second time scale
  where the specification's example reads 10 days; and the write pipeline gives every fact a salience of
  at least 0.45 (base 0.3 + 0.15 for its one or more entities), above the 0.3 floor, so `decay_tenant`
  never ends a fact written through the pipeline. *Mitigation:* embedding callers set `decay_k` (ordin:
  864,000 s); fading is computed from the same curve and shown to the reader rather than acted on; a
  change to what decay removes is left to its own decision.
- **Link-expansion cost on the SQL storage.** Each hop adds a relationship scan over the frontier and a
  fact run per collection; a densely linked entity can reach many neighbours. *Mitigation:* one hop by
  default, at most two; 32 neighbours a hop and 200 graph candidates in total; the benchmark's graph leg
  measures 10,000 facts and 20,000 links on SQLite and SurrealDB against p95 ≤ 200 ms (NFR-P2).
- **Forgetting a fact removes summaries made from it.** Erasure reaches consolidated facts derived from the
  forgotten fact (SA-DELETE-01), so a person may lose a summary they valued. *Mitigation:* kept on purpose
  (a summary may restate the forgotten content); the caller tells the person before forgetting (ordin
  ADR-111).

## Alternatives considered (system-level)

- **Pre-indexed static memory** (bulk-index all documents on a schedule). Rejected — goes stale,
  mixes versions, creates a central privacy surface, and pays the full indexing cost up front. The
  learn-as-you-ask model with a freshness loop is the chosen approach (`agentic-mem.md` §7).
- **Embed memory in model weights (parametric).** Rejected — sacrifices auditability, deletion, and
  cheap updates, all of which the governance and freshness requirements demand.
- **No dedicated service; memory as a library inside each agent.** Rejected — no shared,
  consistently-governed, per-user-isolated store; every agent would reinvent write/forget/audit.

## Dependencies

- **Broker** — authenticates as the user, injects the OIDC token, reads source systems on the
  user's behalf, runs the system allowlist and broker-side audit.
- **OIDC Identity Provider** — issues tokens and exposes discovery/JWKS.
- **Memory store** — the persistence engine (SurrealDB candidate).
- **Embedding / reranker / LLM providers** — for the async write/maintenance path.
- **Observability stack** — logs, metrics, traces sink.
- **Active Practice Pack** — supplies the build/test/lint toolchain.

## Rollout and Rollback

- **Rollout.** Ship as a single self-contained binary / container with the store embedded in-process;
  start single-node; register `memory` as an allowlisted endpoint in the broker. Begin with heuristic control and conservative write-gate
  thresholds. Follow the staged build order from `good-mem.md` §14 — crawl (working, honest memory) →
  walk (stays true over time) → run (gets smarter) — each stage independently shippable and gated by
  the evaluation harness.
- **Rollback.** The service is stateless apart from the store; a bad release rolls back by redeploying
  the prior image. Because writes are non-destructive by default (supersession, not overwrite) and
  the store is the durable system of record, a rollback does not lose memory. Store schema migrations
  are dry-run before apply and shipped with a down path; applying a migration against a shared store
  is an explicit user action, not part of an automated deploy.
