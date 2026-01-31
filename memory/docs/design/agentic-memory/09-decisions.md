# 09 — Architectural Decisions, Assumptions & Open Questions

> **Mode:** draft · **Revision:** 0.12.0 · **Last updated:** January 2026

## Architectural Decisions

### ADR-001: OIDC bearer-token authentication with broker-injected identity
- **Decision.** `memory` authenticates every request via an OIDC-issued JWT bearer token, validated
  against the configured issuer's JWKS; identity is taken from a token claim and mapped to a Scope.
- **Context.** `memory` sits behind the broker, which authenticates as the end user. Identity
  must cross a service boundary without `recall` ever holding raw credentials, and the agent's
  sandboxed script must not be able to construct or widen its own identity.
- **Consequences.** Standards-based and IdP-agnostic; `memory` stays credential-free; per-user
  isolation keys off a verifiable claim. Requires reachable OIDC discovery/JWKS; the exact claim set
  is provider-dependent (OQ-IDP).
- **Alternatives considered.** Static API keys (rejected — no per-user identity, theft risk); mutual
  TLS only (rejected — authenticates the broker, not the end user); bespoke signed headers (rejected
  — reinvents OIDC, harder to integrate).

### ADR-002: Bi-temporal knowledge graph; supersession instead of overwrite
- **Decision.** Facts are stored with two timelines (validity interval + ingestion time); a changed
  fact ends the old fact's validity and records the successor — never a destructive overwrite.
- **Context.** Facts evolve; the named production failure mode is "a high-relevance fact becomes
  confidently wrong when circumstances change". History must remain queryable.
- **Consequences.** Free history, clean contradiction handling, and the temporal/knowledge-update
  quality the concept notes target. Costs more storage and a supersession step in maintenance.
- **Alternatives considered.** Overwrite-in-place (rejected — loses history, silent staleness);
  keep-both-flat (rejected — accumulates contradictions retrieval cannot adjudicate).

### ADR-003: Hybrid multi-model store — embedded SurrealDB
- **Decision.** Use one store providing graph, vector, and keyword retrieval over rich bi-temporal
  edges; the chosen engine is **embedded SurrealDB** (committed via ADR-009). The benchmarking spike
  now *validates* that SurrealDB meets the latency/scale targets rather than selecting between
  engines, with a documented fallback if it does not (OQ-STORE).
- **Context.** Real memory queries need semantic + exact-term + relational signals together, and
  edges must carry validity/confidence/source. Assembling three separate systems adds integration and
  consistency burden.
- **Consequences.** One engine simplifies operations and makes the bi-temporal model natural; couples
  capabilities into one blast radius and depends on a younger engine — hence the validation spike
  (OQ-STORE). **If the spike fails**, the fallback (dedicated graph DB + separate vector index) is a
  **rearchitecture, not a config swap**, and it **reopens ADR-009** — the Rust choice is justified
  primarily by in-process SurrealDB embedding. OQ-STORE is therefore load-bearing for the language
  commitment and should run before any Rust scaffolding.
- **Alternatives considered.** Dedicated graph DB + separate vector index + search engine (rejected
  for v1 — integration cost, cross-store consistency); vector-store-only (rejected — no relationships,
  poor contradiction handling).

### ADR-004: Separate fast LLM-free read path from async write/maintenance path
- **Decision.** Retrieval is synchronous and makes no LLM calls; extraction, consolidation, and
  re-embedding run asynchronously behind a durable queue.
- **Context.** Interactive latency budgets (~200 ms) cannot absorb LLM/embedding calls; writes must
  be retry-safe and must not block reads.
- **Consequences.** Predictable read latency and resilient writes; introduces eventual consistency
  (a just-written fact may not be immediately retrievable) and a queue to operate.
- **Alternatives considered.** Synchronous write-and-index (rejected — LLM latency on the hot path,
  fragile under retries).

### ADR-005: Two-stage retrieval — multi-signal recall then cross-encoder rerank
- **Decision.** Stage 1 fuses semantic + keyword + graph recall; stage 2 reranks the top candidates
  with a cross-encoder; then gating and recency weighting.
- **Context.** Multi-signal fusion beats any single signal; cross-encoders give fine-grained
  relevance but are too slow to run over the whole store.
- **Consequences.** Strong relevance within budget; adds a reranking dependency and tuning surface.
- **Alternatives considered.** Single-stage vector search (rejected — misses exact terms and
  relationships, weaker ranking).

### ADR-006: Graceful decay with a salience floor for forgetting
- **Decision.** Forgetting uses gradual, reinforcement-resettable decay plus TTL on long-tail
  entries, protected by a salience floor; confidence-linked decay fades uncertain facts faster.
- **Context.** Unbounded memory drowns retrieval and bloats storage; crude hard-TTL deletes important
  but rarely-touched facts.
- **Consequences.** Bounded storage and lower retrieval noise while high-importance facts survive
  disuse; requires tuning the decay curve and the floor against real usage.
- **Alternatives considered.** Hard TTL only (rejected — over-forgets); never forget (rejected —
  unbounded growth, contradiction build-up).

### ADR-007: Heuristic memory control at launch
- **Decision.** Memory operations (store / retrieve / update / consolidate / discard) are governed by
  heuristic rules at launch; prompted or learned (RL) control is deferred.
- **Context.** Heuristics are predictable, debuggable, cheap, and auditable — the governance layer
  needs auditability. Learned control is powerful but opaque.
- **Consequences.** Fast to build and reason about; may leave quality on the table that learned
  control could capture later. Instrument to find those gaps.
- **Alternatives considered.** RL-based control from day one (rejected — complexity and opacity
  outweigh launch value); fully LLM-decided operations (rejected — cost and unpredictability).

### ADR-008: Write-gate trust validation and untrusted-data separation
- **Decision.** Every write passes a trust-scoring gate that may quarantine or reject untrusted/
  instruction-like content; externally-sourced content is data to remember, never instructions.
- **Context.** Memory-poisoning attacks via ordinary content have very high reported success rates;
  an agentic-search system that learns from documents is directly exposed.
- **Consequences.** Materially harder to poison; adds a calibration burden (too strict rejects good
  facts, too loose lets poison through) and a quarantine path to operate.
- **Alternatives considered.** Trust-all writes (rejected — unsafe); read-time filtering only
  (rejected — poison already resident; defence in depth needs the write gate too).

### ADR-009: Rust implementation with embedded SurrealDB
- **Decision.** Build `memory` in Rust and embed SurrealDB **in-process** (SurrealKV or RocksDB
  backend) rather than running it as a separate server; retain the engine abstraction so a remote
  SurrealDB / TiKV deployment is reachable later without a code change.
- **Context.** SurrealDB is Rust-native and **only Rust can embed it in-process** — the Go (and
  other) SDKs are clients that require a separate database process. NFR-MA3 wants a single-binary
  single-node deployment, and the read-path latency budget (NFR-P2/P3) benefits from removing the
  client/server hop.
- **Consequences.** A genuine single self-contained binary, lowest read-path latency, and full native
  access to graph + vector + keyword over rich bi-temporal edges. Also a **testing dividend** — the
  same engine runs in-memory for fast tests (ADR-010). Trades the team's Go familiarity for Rust;
  SurrealKV (pure Rust) is the younger backend and RocksDB carries a C++ build dependency. Auth and
  API rely on Rust crates (`tokio`, `axum`, `openidconnect` / `jsonwebtoken`). **Dependency on
  ADR-003:** Rust is chosen *because* SurrealDB is Rust-native and only Rust can embed it in-process;
  if OQ-STORE fails and the store fallback is taken, the embedded-single-binary rationale collapses (a
  network-hop store would not require Rust) and this decision reopens. **Run OQ-STORE before committing
  Rust scaffolding**, not merely before codegen.
- **Alternatives considered.** Go + SurrealDB-as-a-server (rejected — cannot embed; two processes to
  operate; an extra hop on the hot path). Rust + remote-only SurrealDB (rejected for v1 — forgoes the
  single-binary benefit, though available later for scale-out). Other languages (rejected — weaker fit
  with the Rust-native store).

### ADR-010: Outside-in, integration-first BDD test strategy with containerised dependencies
- **Decision.** Test **outside-in**: drive behaviour through the public HTTP API using **BDD**
  scenarios (Given/When/Then) as the primary suite, backed by **real dependencies in test containers**
  — SurrealDB in server mode, the work queue, and **Dex as the OIDC provider** for auth scenarios,
  with stand-ins for the embedding/LLM providers. **Optimise container sessions** for fast cycles:
  reuse/singleton containers across a scenario suite, run the fastest inner loop against SurrealDB's
  **in-memory** backend, and execute scenarios in parallel with per-scenario scope isolation. Pure
  unit tests are reserved for algorithmic cores (decay maths, ranking fusion, entity-resolution rules).
- **Context.** `memory`'s correctness is mostly *emergent from how components interact* — auth→scope→
  retrieval, async write→store→retrieval, supersession over time. Mock-heavy unit tests miss that and
  drift from real dependency behaviour; the team wants confidence the assembled system behaves, with
  fast feedback.
- **Consequences.** High-fidelity tests that exercise real wiring and catch integration drift; BDD
  scenarios double as living documentation and map directly to the spec's Gherkin (C3). Containers are
  heavier than mocks, so the session optimisations above are required to keep cycles short — and the
  embedded-Rust decision (ADR-009) pays off here, since the same store runs in-memory for speed and
  containerised for fidelity. Dex gives a real OIDC token issuer in tests without depending on a
  hosted IdP (the production IdP stays configuration — OQ-IDP).
- **Alternatives considered.** Mock-everything unit pyramid (rejected — misses the cross-component
  behaviour that is most of `memory`'s risk; mocks drift from real dependencies). End-to-end-only
  against a deployed stack (rejected — slow, flaky, hard to seed state). A hosted IdP for auth tests
  (rejected — network dependency and flakiness; Dex-in-a-container is hermetic).

### ADR-011: Bridge-model tenancy — namespace-per-tenant + logical team/user scoping
- **Decision.** Isolate tenants (organisations) with a **SurrealDB namespace each** — a hard boundary
  with no cross-tenant query, traversal, or vector neighbour — and segment **teams and users
  logically within** a tenant via record-level `PERMISSIONS` keyed on the authenticated membership
  claims, with a Fact-level **visibility** (user-private | team-shared | tenant-shared). Where a team
  needs physical isolation, give it its own **database within the tenant namespace**.
- **Context.** Tenants must never commingle (compliance, plus the graph "walk into the wrong
  neighbour" traversal risk and vector-ANN leakage on a shared index), but intra-tenant sharing and
  team-level consolidation are features, not leaks. SurrealDB's namespace → database → record
  hierarchy supplies exactly these two seams, and its vector index is per-table, so a structural
  tenant boundary also isolates ANN candidates.
- **Consequences.** Hard cross-tenant isolation with clean per-tenant backup / export / erasure
  (drop the namespace), plus efficient sharing and consolidation inside a tenant on one index. Two
  mechanisms to reason about (structural + logical); correct record-permission rules and
  membership-claim handling become security-critical and a first-class test target (ADR-010). Scales
  out by moving a large tenant's namespace to its own instance / cluster without a data-model change
  (ADR-009 abstraction).
- **Alternatives considered.** Per-user physical graphs (rejected — kills sharing / consolidation,
  explodes object count). Pure pool / tenant-column-only (rejected — traversal and vector-ANN
  leakage, weaker compliance and deletion story). Silo-per-team everywhere (rejected as the default —
  blocks cross-team sharing and multiplies objects; retained only as the per-team escape hatch).

### ADR-012: The read path is LLM-free but performs two model inferences (query embed + rerank)
- **Decision.** NFR-P1 ("no LLM calls on the read path") is scoped to **LLM** calls. The synchronous
  read path **does** perform exactly two non-LLM **model inferences**: (1) embedding the incoming query
  (semantic recall needs a query vector, which a novel query cannot have cached), and (2) a
  cross-encoder **rerank** of the bounded stage-1 candidate set. Each gets an explicit latency
  sub-budget inside NFR-P2. Extraction is performed by the agent and server-side consolidation is
  dropped (ADR-015); `memory` makes no LLM call at all. The two read-path model inferences
  (query-embed, rerank) are unaffected.
- **Context.** Earlier draft prose described the embedding provider as "off the read path /
  asynchronous" and the reranker as "off the read-path candidate set", which contradicted both the
  architecture (stage-2 rerank is synchronous — `good-mem.md` §7.2) and the need to embed each query at
  read time. Left uncorrected, `/spec` would derive a read-path latency contract with no line item for
  these two inferences.
- **Consequences.** The recall latency budget must allocate sub-budgets for query-embed and rerank; the
  embedding/reranker **hosting choice (local vs remote) becomes a read-path latency decision**, not the
  async "cost tuning" OQ-MODELS implied — a local/in-process embedder is the mitigation if a remote hop
  threatens NFR-P2/P3. Query **reformulation stays optional and A/B-gated** (`good-mem.md` §7.3 warns it
  can hurt), not a fixed stage.
- **Alternatives considered.** Keep the read path fully model-free (rejected — impossible for semantic
  recall, which requires a query vector, and it would forgo the cross-encoder precision `good-mem.md`
  §7.2 shows dominates single-stage retrieval). Move rerank async (rejected — reranking after the
  response is returned defeats its purpose).

### ADR-013: Freshness check is memory-side and conditional on the read path; re-read is asynchronous
> **Superseded by ADR-014 (January 2026).** The memory-side check assumed a centrally-reachable broker; the broker is in fact a per-agent local component, so freshness moves to the agent.
- **Decision.** The freshness loop runs **memory-side**. On the read path `memory` performs only a
  **cheap conditional source-change check** (modification marker / `If-Modified-Since`) by calling the
  broker **as the user**; if the source changed, the actual **re-read and re-extraction are enqueued
  and run asynchronously**, and the answer flags the fact *stale-pending-refresh*. If the broker or
  source is unreachable, the stored fact is returned flagged *unverified-currency*.
- **Context.** Resolves OQ-FRESH-PLACEMENT. A naive "re-read on the read path" would put a source fetch
  (and its latency) inside the ≤200 ms budget; doing nothing would return confidently stale facts (the
  central named failure mode). The conditional-check / async-reread split keeps currency verification
  cheap on the path while still catching change. It requires a re-entrant `recall`→broker call, whose
  shape is named in [08 — Interfaces](./08-interfaces.md) (Outbound interfaces).
- **Consequences.** The recall endpoint's latency contract includes one conditional broker round-trip;
  the broker must expose a conditional source-read (`If-Modified-Since`) callable as the user; reads are
  **eventually-fresh** (a changed source is reflected after the async refresh, not within the same
  call). Pairs with ADR-004 — the refresh rides the existing async queue.
- **Alternatives considered.** Re-read synchronously on the read path (rejected — source-fetch latency
  blows NFR-P2). Broker-side freshness (rejected as default — pushes orchestration back onto the agent
  the API is meant to keep simple; still available as a deployment variant). No freshness check
  (rejected — returns confidently stale facts).

### ADR-014: Recall is a passive memory store — freshness and orchestration are agent-side (supersedes ADR-013)
- **Decision.** `memory` performs **no read-path source-change check, computes no currency verdict, makes
  no outbound broker call, and enqueues no re-read job.** On recall it returns each fact's stored
  provenance (`origin_ref` + `modification_marker`) **conditionally**, gated by a request parameter, so
  the agent — which is co-located with its broker and holds document access — runs the
  ask → check → update loop itself. The UPDATE leg uses the existing write/supersede endpoints.
  **Supersedes ADR-013.**
- **Context.** ADR-013 placed the freshness check inside `memory`, assuming a centrally-reachable broker.
  In the real deployment the broker is a **per-agent local component** (e.g. Copilot in VS Code on a
  developer's machine); a central `memory` cannot reach it (no routable address, behind NAT, alive only
  while the IDE runs). The memory-side check is therefore unreachable and would degrade every fact to
  *unverified-currency*. ADR-013 itself named the agent-driven approach as the deployment variant; this
  deployment makes it the only viable one.
- **Consequences.** `memory` has no outbound dependency for freshness. Removed: the C5 Freshness Checker,
  the `BrokerClient` port + `HttpBrokerClient` adapter, the `MEMORY_BROKER_URL` config, the `ReReadSource`
  job kind, and the `currency` field on the recall response. Added: conditional source provenance on the
  recall (and get-fact) responses. Reads are no longer "eventually-fresh via recall" — freshness is the
  agent's responsibility. The embedding and reranker read-path inferences are retained (model inferences,
  not orchestration).
- **Alternatives considered.** Keep ADR-013 memory-side check (rejected — unreachable broker). Keep it
  behind a deployment flag (rejected per the directive — `memory` makes no broker calls). Have the broker
  call `recall` to push freshness (rejected — inverts the topology and re-adds orchestration to `memory`).

### ADR-015: Recall is LLM-free — extraction and consolidation are the agent's responsibility
- **Decision.** `memory` holds **no LLM dependency**. The write path performs **no LLM extraction** —
  callers submit structured, agent-asserted facts. **Server-side episodic→semantic consolidation is
  dropped**; the agent distils insights with its own LLM and writes them back as agent-stated
  `consolidated` facts. Removed: the `LlmClient` port, the `HttpLlmClient` adapter, `MEMORY_LLM_URL` /
  `MEMORY_LLM_API_KEY`, the `Consolidate` job kind, and `InsightCandidate`. The embedding and reranker
  inferences (non-LLM, ADR-012) are **retained**.
- **Context.** Completes ADR-014's passive-store direction. The agent already runs an LLM; extraction
  and synthesis are reasoning that belongs with the agent, not the memory store. Removing the LLM
  simplifies deployment (no LLM provider, endpoint, or key) and shrinks the ingest trust surface (no
  model interpreting untrusted source content on the way in).
- **Consequences.** `POST /v1/memories` accepts only structured assertions (breaking). The write
  pipeline keeps its non-LLM steps (filter, canonicalise, entity-resolve, score, PII scan, write-gate,
  embed+persist). The maintenance worker keeps supersession, graceful decay, re-embed (via the embedding
  provider), and verifiable hard delete, and loses consolidation. The `consolidated` `MemoryClass` and
  `derived_from` remain for agent-written insights. recall is generative-model-free; only the retrieval
  embedding/reranker model inferences remain.
- **Alternatives considered.** Relocate consolidation to a separate worker (rejected — adds a deployable
  and keeps an LLM in the system). LLM-free heuristic consolidation (rejected — weak insights). Keep
  server-side LLM extraction/consolidation (rejected — contradicts ADR-014's passive-store direction).

### ADR-016: One service core, two transport edges (REST + MCP); MCP over streamable-HTTP reusing OIDC
- **Decision.** Extract the per-operation orchestration — authenticate→`ScopeContext`, authorise the
  operation class, rate-limit, idempotency, invoke the owning component, write the audit record, and
  classify any failure to an X1 `code` — into a transport-agnostic **Service Layer**. Expose it through
  two thin edges in **two binaries**: the existing **REST** edge (`recall`) and a new **MCP** edge
  (`memory-mcp`). The MCP edge runs over **MCP streamable-HTTP** and carries the same
  `Authorization: Bearer <OIDC JWT>` the REST edge validates, so the Auth & Scope component is reused
  verbatim. Both binaries construct their state from the same bootstrap path; neither holds a private
  copy of the orchestration. Introduced by RFC 01.
- **Context.** Agents increasingly consume tools over MCP; `recall` must offer an MCP interface without
  forking the service. The orchestration that makes `recall` correct and governable (auth, scope, rate
  limiting, idempotency, audit) must exist **once** — a second copy is a security and correctness
  hazard. `memory` is a central, multi-tenant, OIDC-authenticated store; the per-agent-local component
  is the broker, not `recall`. A networked, bearer-authenticated MCP edge therefore fits its nature and
  reuses all governance unchanged.
- **Consequences.** A single source of truth for auth/scope/rate/idempotency/audit; the REST contract
  is unchanged (the HTTP edge becomes a thin adapter, proven behaviour-preserving by the existing BDD
  suite passing unmodified); MCP adds a transport only — no new outbound calls and no LLM (ADR-014 /
  ADR-015 hold); two deployable manifestations from one core. Costs: a second binary to build,
  configure, and ship, and an MCP transport library joins the stack (the exact crate is an open
  question resolved at codegen).
- **Alternatives considered.** MCP over **stdio** as a per-agent local subprocess (rejected — no OIDC
  header; would require a new ambient/handshake identity model and weaken the multi-tenant posture; a
  local stdio shim, if ever needed, is the broker's responsibility). MCP handlers **inside the existing
  binary** alongside the HTTP handlers (rejected — does not deliver "separate binary / different
  manifestations" and risks shared-process coupling). **Duplicating** the edge logic for MCP (rejected
  — the exact divergence hazard this decision exists to prevent).

### ADR-018: Per-tenant erasure is a predicate-scoped delete with proof; the namespace drop is reserved for single-tenant namespaces
- **Decision.** Per-tenant erasure is `MemoryStore::erase_tenant(tenant)`: a predicate-scoped delete of
  every row carrying that tenant, across every table in the namespace the tenant occupies, verified by
  a post-delete re-count and reported as a `TenantErasureProof` (per-table removed counts, total, and a
  digest over them). The namespace drop (`drop_tenant_namespace`, `REMOVE NAMESPACE`) is retained only
  as the single-tenant-namespace mechanism and **refuses to run** anywhere else: a namespace counts as
  exclusive to one tenant when the deployment profile declares it so **and** the tenant→namespace
  mapping resolves the tenant onto its own name, and both must agree before the drop is issued.
- **Context.** ADR-011 makes the namespace the tenant boundary, and three rows of
  [06 — Regulatory](./06-regulatory.md) rest on the equivalence *erasing one tenant = dropping one
  namespace*: right to erasure and purpose limitation cite ADR-011 by name, and auditability depends on
  it for the per-tenant trail. RFC 03's topology stage maps many tenants onto one namespace, which
  breaks that equivalence in the worst possible direction — the erasure call a data subject invokes to
  exercise a right would destroy every co-resident tenant's data, irreversibly. The read-side half of
  the same defect was closed by putting `owner.tenant = $ctenant` on every read filter; the destroy side
  needs the same move, because a guarantee that lives in the topology disappears when the topology
  changes, and a guarantee that lives in the predicate does not.
- **Consequences.** Erasure is correct under both topologies and no longer depends on the tenant→
  namespace mapping, so the topology becomes a provisioning choice rather than a compliance
  dependency. Tenant isolation is now two guarantees rather than one, and they are not equally strong:
  **structural** isolation (one namespace per tenant) is enforced by the engine, **predicate** isolation
  (co-resident tenants, separated by `owner.tenant` on every read filter and on the erasure predicate)
  is enforced by the correctness of a query fragment. 06-regulatory states which of the two a deployment
  has instead of claiming the stronger one for all of them. The proof also changes shape: a
  whole-tenant erasure reports per-table counts and a zero-remaining verification rather than the
  record-id digest `hard_delete` returns, because the claim being proved is *nothing of this tenant
  remains*, which the verification pass checks directly. The cost is that erasure scans each table on
  the tenant predicate instead of removing a namespace in one structural operation.
- **Alternatives considered.** Keep the namespace drop and forbid co-residency (rejected — that makes
  the topology stage unreachable, and the tenant-population ceiling is the reason it exists). Give each
  co-resident tenant its own **database** inside the shared namespace and drop that instead (rejected —
  it restores the per-tenant object count the topology stage exists to remove, and the tenants that do
  share a database still need the predicate path). Erase by moving the tenant's rows to a tombstone
  table (rejected — a table still holding the rows is retention, not erasure, and a proof over it would
  be false).

## Assumptions

| ID | Area | Assumption | Rationale |
|---|---|---|---|
| ASSUMP-IDENTITY | Auth | Identity is supplied by the broker; `memory` validates the token and never handles raw user credentials. | Matches the assumed broker model (`agentic-mem.md` §7.2). |
| ASSUMP-OIDC | Auth | OIDC JWT is the authentication standard. | Standards-based, IdP-agnostic way to convey user identity across a boundary (ADR-001). |
| ASSUMP-TENANCY | Tenancy | Memory is segmented Tenant → Team → User via the bridge model (namespace-per-tenant + logical scoping). | Committed via ADR-011; supports hard cross-tenant isolation and intra-tenant sharing. |
| ASSUMP-STORE-CAP | Storage | The store provides graph + vector + keyword retrieval over rich bi-temporal edges; embedded SurrealDB is the committed engine. | Required capability set (`agentic-mem.md` §4–§5, §9); committed via ADR-003/ADR-009; spike validates (OQ-STORE). |
| ASSUMP-ASYNC | Architecture | Read path is synchronous and LLM-free; write/maintenance is asynchronous. | Latency and retry-safety (ADR-004). |
| ~~ASSUMP-LANG~~ | Tech | _Superseded — language is now a committed decision, not an assumption._ | Promoted to **ADR-009** (Rust + embedded SurrealDB). |
| ASSUMP-QUEUE | Tech | A durable work queue backs the async path; concrete product deferred. | Decoupling and retry-safety; product choice is non-architectural (OQ-QUEUE). |
| ASSUMP-MODELS | Tech | Embedding, reranker, and extraction/consolidation LLM are external, configurable providers. **Query embedding and rerank run on the read path; fact-content embedding and the extraction/consolidation LLM run off it** (ADR-012). | Keeps the read path **LLM-free** (not model-free); providers are configuration (OQ-MODELS). |

## Open Questions

Each has a defensible default that lets `/spec` proceed. **One exception:** OQ-STORE is non-blocking
for *spec authoring* but **load-bearing for the Rust / embedded commitment** (ADR-009) — run its spike
before Rust scaffolding, since a failure reopens the language choice. The rest are confirm-early-if-you-can.

| ID | Question | Plain-English | Options | Impact | Blocking? |
|---|---|---|---|---|---|
| OQ-STORE | Does embedded SurrealDB meet the latency/scale targets, or is a fallback needed? | Does the chosen all-in-one database actually keep up, or do we need a backup plan? | Validate and keep (expected) · fallback to graph DB + vector index | Engine is committed (ADR-003/009); the spike validates rather than selects. **Load-bearing for ADR-009** — a spike failure reopens both ADR-003 and ADR-009 (the language choice). | No for `/spec` authoring; **yes for the Rust / embedded commitment** — run before scaffolding |
| OQ-VOLUMES | What are the target corpus size, tenant counts, and SLOs? | How much memory, how many users, and how fast must it stay? | Corpus size and SLOs remain TBC with deployment context. **Tenant count now has a named context:** the *consumer tenancy profile* — 10,000 and 50,000 single-user namespaces — and a specified instrument to measure it, the Tenancy Benchmark Harness (spec `docs/spec/components/tenancy-benchmark.md`, Playbook Step 6c). | Pins the scale/latency NFRs; defaults hold until set. The consumer-profile half of this question is answered by the benchmark run, not by argument: until it runs, the per-namespace footprint and the tenant-population ceiling are both unmeasured. | No |
| OQ-IDP | Which OIDC provider, and the exact claims/audience the broker emits in production? | Which login system issues the tokens in production, and what's inside them? | Any compliant OIDC provider (configuration); **Dex** used for testing (ADR-010) | `memory` is built IdP-agnostic; production values are config, tests use Dex. | No |
| OQ-CONSOLIDATE-CADENCE | When does consolidation run — on write, on a timer, or on idle? | How often do we distil raw memories into durable knowledge? | Idle-biased (default) · timer · on-write | Affects freshness of semantic facts vs background cost. | No |
| OQ-QUEUE | Which durable-queue product backs the async path? | What technology carries background work? | Store-backed · NATS · other | Operational, not architectural; deferrable. | No |
| OQ-MODELS | Which embedding / reranker / LLM providers, and how hosted? | Which AI models do the work, and where do they run? | External providers (configuration); local/in-process embedder as a latency option | **Read-path latency** (query-embed + rerank) *and* async cost (extraction/consolidation); local vs hosted for the embedder/reranker is a read-path latency decision (ADR-012). | No |

**Resolved**

- **OQ-LANG-EMBED** — *Is single-binary in-process store embedding a hard requirement?* **Resolved
  January 2026:** yes — committed to **Rust + embedded SurrealDB** (ADR-009), giving a genuine
  single-binary single-node deployment (NFR-MA3).
- **OQ-TENANCY** — *Per-user only, or team / organisation sharing?* **Resolved January 2026:** adopt the
  **bridge model** (ADR-011) — namespace-per-tenant hard boundary with logical Team / User visibility
  within. Residual sub-question for `/spec` / the store spike: the threshold at which a team is
  promoted to its own database (the physical-isolation escape hatch).
- **OQ-FRESH-PLACEMENT** — *How much of ask→check→refresh→answer runs inside `memory` vs the broker?*
  **Resolved January 2026:** **memory-side**, via the split committed in **ADR-013** — a cheap
  conditional source-change check on the read path (through the broker, as the user) plus an
  **asynchronous** source re-read/re-extract off it. The outbound `recall`→broker shape is named in
  [08 — Interfaces](./08-interfaces.md).

### ADR-019: Write authority is a function of the fact class asserted, carried as a token claim, and defaulting open
- **Decision.** A `remember` request may carry an optional `fact_class` label, and a token may carry an
  optional `memory_fact_authority` claim naming the classes it may assert and, per entry, the one source
  it may attribute them to. The check is applied in exactly one place — the Service Layer's `remember`
  chain, after authorisation and before the write is enqueued — so both transport edges inherit it. The
  class vocabulary belongs entirely to the caller: `memory` compares two opaque strings and interprets
  neither. Three sub-decisions carry the weight. **The label is a first-class request field**, not a
  convention read out of `content`, because a service should not depend on a key inside a document it
  does not own. **Authority is carried as a token claim**, not stored, because the authority decision
  belongs with whoever issues tokens; revocation is by reissue. **An absent claim grants every class**,
  and the absence is reported on a counter rather than left silent.
- **Context.** The Companion Platform's own ADR-007 places this control here deliberately, so that it
  lives in one testable place rather than being re-derived in every component that writes. Until now
  `memory` could authorise a caller to write or not to write and nothing in between: `Op` has three
  values and authorisation is a field lookup, so there was nowhere to ask *what* is being asserted. A
  sibling integration run wrote two facts its own design says have different authors and different
  consequences, and received two identical `202`s. The gap survived design review on both sides because
  both corpora already had something called a write gate: ADR-008's gate scores *whether content is
  trying to give the system instructions*, which is a different question from *whether this principal
  may say this*. Adding a fourth `Op` was the obvious shape and was rejected for a reason visible only
  in the audit trail — a refusal raised inside the authorisation step writes no audit record, and the
  requirement is that the refusal is audited.
- **Consequences.** `memory` gains a second authorisation axis without gaining any domain knowledge:
  it holds no vocabulary of classes and cannot be asked to reason about what a fact means, which keeps
  the LLM-free constraint intact. The control's honest boundary is that it governs **labelled**
  assertions: a caller that omits `fact_class` is asserting nothing classed and is admitted on the write
  scope alone, so what a consumer gains is the guarantee that a fact it reads back *as* class `A` was
  written by a principal entitled to assert `A` — not that no unlabelled equivalent exists. Closing that
  would require inferring class from content, which ADR-015 forbids. The open default keeps every
  existing deployment working and makes the control opt-in, which is precisely how it came to be missing
  in the first place; the mitigation is a counter a deployment can alarm on, not a different default,
  because a silent safe default and a silent unsafe default fail the same way. Two smaller consequences
  follow from the enforcement point: a refused write consumes rate-limit budget, which makes probing
  cost something, and it consumes no idempotency key, so the same key succeeds once the token is fixed.
  The audit row grows two columns (`client`, `fact_class`) and the calling service identity becomes
  available for the first time — the trail previously could not distinguish two services acting for the
  same user.
- **Amended January 2026 (RFC 05).** The decision as first implemented compared the label and then
  discarded it, which left the guarantee weaker than it reads: the gate judged one string while a
  consumer, having no stored class to read, acted on whatever label the caller had put inside
  `content` — a string `memory` never compared against anything. The authorised class is therefore
  **stored on the fact** and returned on every read, and carried on a quarantined write so a reviewer
  sees what a held write claimed to be. This changes none of the decision above; it makes the decision
  observable to the reader it exists to protect. The class remains opaque, remains unindexed, and is
  deliberately **not** filterable — a consumer filters client-side on a field it now receives, and
  server-side selection stays a separate, measurable change. A superseding write carries its own class
  rather than inheriting its predecessor's, because a supersession is a new assertion that passed the
  gate with its own token.
- **Alternatives considered.** A stored authority table (rejected for now — immediately revocable
  without reissuing a token, but it is a migration, an admin surface and a cache, and the enforcement
  point is designed so a table can replace the claim later without moving the check). Reading the class
  from inside `content` (rejected — makes the key name a contract without documenting it as one, and
  couples `recall` to a caller's document shape). Denying by default on an absent claim (rejected —
  safe, and it stops every current caller on upgrade). A general policy engine with an expression
  language (rejected — a different risk profile and a much larger change; this is a comparison of two
  strings). Leaving the rule to each caller (rejected — the sibling has already done it once by making
  the write inexpressible in one component's type, which closes the accidental route and not the
  deliberate one, and has to be re-done correctly in every future component that writes).

### ADR-020: Facts carry a collection; stage-1 fuses per-collection ranked lists by reciprocal rank; recall accepts typed sub-queries

- **Status.** Accepted January 2026 (ordin ADR-110, decision 90, through the ordin `/spec` HLD-impact-pass; the decision is the ordin user's of January 2026, "yes, read-only — proceed with recommendations"). Revision 0.11.0.
- **Decision.** Four parts, one change. **(1) Collections.** A fact, entity, relationship and source may carry a `collection` label chosen by the caller — `""` is the default collection, which every existing row belongs to. The label partitions one scope's facts into sets searched side by side; it is never an access rule, and the read filter alone decides what a caller can read. **(2) Filter first.** A recall may name the collections to search; the restriction is applied before every stage-1 signal, never after a global top-K. **(3) Rank fusion.** Stage-1 runs each signal separately within each collection and fuses the ranked lists by reciprocal rank (`Σ w / (60 + rank)`, a small bonus for ranks 1–3), keeps each collection's best matches through a per-collection quota, and caps the candidate set by fused rank — replacing the as-built cap, which kept the first `stage1_k` facts in fact-id order. The reranker and recency steps are unchanged; when the reranker is unavailable, the fused rank orders the result. **(4) Typed sub-queries.** A recall may carry up to four sub-queries, each marked keyword (`lex`) or meaning (`vec`), written by the agent; the first counts double, every `vec` sub-query is embedded in one call, and one rerank runs against the first. A plain query keeps working as before. A content check that reports what the write gate would find, without writing, is added for the in-process ordin caller only.
- **Context.** ordin (ADR-110) lets a workspace read several teams' knowledge folders and recall from them together. Folders differ in size by two orders of magnitude, and raw scores are not comparable across them: BM25 is normalised by the maximum of its own result set, so the largest folder sets the scale for everyone, and a global top-K followed by a filter starves the small folder (the over-fetch is clamped to 1,024). Reviewing the stage-1 merge for this showed a second defect: when the three signals together surface more than `stage1_k` facts, the cap keeps them in fact-id order — oldest first — not by relevance. The techniques come from qmd, an open-source on-device search engine (MIT), whose collections are searched together in the same way; no qmd code is used.
- **Consequences.** Stage-1 cost becomes lists × collections runs, and a collection run under a collection filter uses the filter-first exact vector strategy, O(visible facts of that collection) — bounded by the per-collection cost risk in `10-risks.md` and measured by the benchmark's collection leg. Entities, relationships and sources carry the collection too, so one collection's knowledge can be erased by predicate (`erase_collection`) while an entity or source a fact of another collection still names is kept. Migration `0007` adds the field, backfill and index to four tables and the field to `quarantine`. The retrieval regression suite holds for plain queries whose stage-1 union fits in `stage1_k`; above that, the candidate set changes by design. On the degraded path abstention now gates on the fused rank, not on vector similarity. The memory component still makes no LLM call on the read path (ADR-012, ADR-015).
- **Alternatives considered.** *Max-normalised score fusion* (rejected — the normalisation constant is set by the largest collection, the problem being solved). *A global top-K, then a collection filter* (rejected — starves small collections behind the 1,024 clamp). *Query rewording by a model on the read path* (rejected — stays behind ADR-012's flag; the agent writes typed sub-queries itself). *One recall per collection, merged by the caller* (rejected — one embed and one rerank per collection exceeds the read-path budget). *A separate scope per collection* (rejected — scope is the access boundary; a reader of several folders would need several identities).

### ADR-021: Facts carry the entities and links the agent states; recall matches entities from the query and follows live links one hop; consolidation is the caller's; fading is shown, not hidden

- **Status.** Accepted January 2026 (ordin ADR-111, decision 91, through the ordin `/spec` HLD-impact-pass; the decision is the ordin user's of January 2026, "yes, every agent gets memory — apply recommendations"). Revision 0.12.0.
- **Decision.** Five parts, one change. **(1) The graph is written.** A remember may carry the entities the fact is about (a name and other names) and the links the agent states between them (`from`, `kind`, `to`, a confidence, and `ended` for "no longer true"). Entities are resolved by the existing three tiers; link kinds are free text, normalised, and merged with an existing kind by the same tiers. A link is written only when the write gate admits its fact, records which facts asserted or ended it, and closes when no live fact still asserts it; erasure removes a link left with no asserting fact. Names and kinds pass the same personal-data scan and instruction check as content. **(2) The graph is used.** Recall matches the entities a query names by exact normalised name or alias, without the caller passing an entity filter, and adds two ranked lists to stage-1's reciprocal rank fusion (ADR-020): facts naming a matched entity, and facts about entities reached along live links — one hop by default, at most two, bounded per hop and in total. A link is followed only through a fact the reader can read. Recall returns the links it followed. **(3) Source trust in the final score.** `final = rerank × (1 + recency) × (1 + w·(trust − 0.5))`, w = 0.1, so a fact from a trusted source ranks above an otherwise equal document fact. **(4) Consolidation is the caller's.** A remember of class `consolidated` may name 2–64 facts it summarises (`derived_from`); the memory component still writes no consolidated fact of its own (ADR-015). **(5) Visibility.** A fact's versions, a summary's origins, a scope's entities and one entity with its facts and links are read operations; each returned fact says whether it is **fading** (an episodic fact in the last 20% of the time decay allows before its prune point); keeping a fact, counting, erasing and merging entities, and correcting or forgetting one link are operations for the in-process ordin caller only.
- **Context.** ordin (ADR-111) makes agentic memory a core capability that every agent session uses and the person can see and govern in full. The store has always declared relationships and a graph signal, but nothing wrote a relationship, and the graph leg ran only when a caller passed an entity id, so the graph was not used. ordin's knowledge agents will build on the same model, so the model must be complete before them.
- **Consequences.** Migration `0008` adds `relationship.replaces`, a live-link index, the `relationship_fact` table, `entity.names_norm` with an index, a backfill marker, and three held-write fields on `quarantine`; entities written before it receive their normalised names from a Rust pass at start, and recall runs without the graph lists until that pass ends. Stage-1 gains up to 1 + hops runs per collection; the whole read path stays inside p95 ≤ 200 ms (NFR-P2) at 10,000 facts and 20,000 links with one hop, measured by the benchmark's graph leg on SurrealDB and on ordin's SQL storage. Forgetting a fact still removes the summaries derived from it (SA-DELETE-01). Two as-built facts are recorded in `10-risks.md`: the decay time unit and a salience heuristic under which decay never prunes. No model call is added anywhere (ADR-012, ADR-015).
- **Alternatives considered.** *Extracting entities and links inside memory with a model* (rejected — ADR-015; the agent states them). *Graph syntax of one engine for traversal* (rejected — ordin's SQL storage must behave the same, ordin ADR-101). *Fuzzy entity matching at read time* (rejected — a similarity scan over every name per query exceeds the stage-1 budget). *A separate job per link* (rejected — a link could outlive or precede its fact's gate decision). *A server-side consolidation duty* (rejected again — ADR-015; the caller has the model). *Keeping summaries when an origin is forgotten* (rejected — a summary may restate the erased fact).
