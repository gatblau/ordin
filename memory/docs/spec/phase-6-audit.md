# Phase 6 — Self-Audit

> **Spec set:** `memory` (agentic memory service) · **Mode:** greenfield
> **derivedFromHld:** 0.12.0 · **Source HLD:** `docs/design/agentic-memory/` · **Authored:** January 2026 · **Amended:** January 2026 (RFC 01, ADR-014; RFC 02, ADR-015), January 2026 (RFC 01-MCP, ADR-016), January 2026 (RFC 02-consumer-tenancy-profile, pass 1)

This is the blocking gate that promotes the spec set from *Draft* to *Approved*. Each checklist item
is ticked against the generated specs (Phases 1–5). **Verdict: PASS** — every item is `[x]`.

**RFC 01 / ADR-014 re-audit (January 2026).** ADR-014 supersedes ADR-013: `memory` is a passive store,
freshness is agent-side. C5 Freshness Checker is **retired**; the memory-side source-change check, the
`Currency`/`FreshnessChecker`/`BrokerClient`/`SourceState` types, the `ReReadSource` job, and
`MEMORY_BROKER_URL`/`MEMORY_FRESHNESS_*` are removed; `RankedFact` drops `currency` and gains a
conditional `source` (`include_provenance`). The checklist below was re-run against the amended set
(seven live component specs + retired C5) and still passes; the affected items are annotated.

**RFC 02 / ADR-015 re-audit (January 2026).** `memory` is **LLM-free**: C4 drops LLM extraction (a
`remember` payload carries a structured agent-asserted `content` object the pipeline wraps directly);
C7 drops server-side consolidation (five duties → four). Removed: the `LlmClient` trait, `HttpLlmClient`,
`InsightCandidate`, the `Consolidate` job kind, and `MEMORY_LLM_*` + the consolidation config (one
interval key renamed `MEMORY_MAINT_MAX_INTERVAL_SECS`). PII detection is now in-process deterministic.
The checklist was re-run against the LLM-free set and still passes; affected items are annotated.

```
[x] Every entity has a complete data model with types and constraints.
[x] Every action has defined inputs, outputs, numbered steps, and errors.
[x] No banned phrases remain (see banned-phrases.md).
[x] Every component has ≥3 Gherkin acceptance criteria (happy, edge, error).
[x] Every component has an error table with ≥2 rows.
[x] Every cross-component interaction is documented on BOTH sides.
[x] Build order (Phase 2B) is a valid DAG — no circular dependencies.
[x] Every config value / env var listed with type, default, required flag, and owner component.
[x] Every spec is self-contained — implementable from its section alone.
[x] Assumptions register is complete; open questions are truly blocking only.
[x] Example I/O provided for every component with non-trivial logic.
[x] Shared types defined once in Phase 2C, referenced by name but duplicated into each "Shared Context".
[x] Security addressed for every entry point (HTTP handler, queue consumer, scheduler).
[x] Performance targets stated for every latency-sensitive component.
[x] All specifications use the active locale (en-GB).
[x] Destructive schema changes surface rollback considerations.
[x] Every Component, Cross-cutting, and Preamble spec carries a `derivedFromHld:` preamble field.
[x] No spec body contains an ADR-shaped paragraph; every system-level trade-off is in HLD 09-decisions.
[x] Every glossary term used in any spec is present in the HLD glossary.
[x] Every spec's `derivedFromHld:` matches the HLD's current `revision:` — no stale phases.
[x] In folder-form HLDs: every revision bump in the HLD has a corresponding `99-changelog.md` line.
```

## Evidence and notes per item

1. **Data models complete.** §2C.2 defines `Fact`/`Entity`/`Relationship`/`Source` with types and
   validation; C1 gives the SurrealDB DDL (`DEFINE FIELD`/`ASSERT`/`DEFINE INDEX`) for every table incl.
   `pii_review`, `embedding`, `embedding_model`. Component-owned tables (`quarantine`, `work_job`,
   `dead_letter`, `maintenance_state`, `idempotency_record`, `schema_migrations`, `audit_log`) each carry
   DDL in their owning spec.
2. **Actions specified.** Every component has numbered Internal Logic with the dependency called, errors
   possible, and what is logged per step.
3. **No banned phrases.** Scan for `etc.`, `and so on`, `as needed`, `as appropriate`, `should be
   robust`, `straightforward`, `obviously`, `properly`, `correctly` returns zero hits across `docs/spec/`.
   ("in-process", "process boundary", and the noun "handle" in `Source.origin_ref` are not banned-verb
   usages.)
4. **Gherkin ≥3.** Component counts: memory-store 6, work-queue 6, auth-scope 7, write-pipeline 10,
   retrieval-engine 7, maintenance-worker 6, http-api-edge 11 (C5 freshness-checker retired — ADR-014;
   maintenance-worker dropped its consolidation scenario — ADR-015).
   Each cross-cutting spec (X1–X13) has exactly 3 (happy/edge/error). All ≥3.
5. **Error table ≥2.** Components: memory-store 7, work-queue 5, auth-scope 5, write-pipeline 9,
   retrieval-engine 6, maintenance-worker 9, http-api-edge 17 (C5 retired). Cross-cutting specs
   that produce client/lifecycle errors carry ≥2 rows (X1, X2, X6, X7, X8, X9, X10, X12, X13). The
   pure-observability concerns **X3 Logging, X4 Metrics, X5 Tracing** and **X11 CORS (off by default)**
   record `N/A — <reason>` per the template's explicit "if a section genuinely does not apply" rule:
   they emit no client-facing error path. This is a documented, justified exception, not an omission.
6. **Both-side interactions.** C8↔C3 (auth), C8↔C6 (recall), C8↔C2 (enqueue), C8↔C1 (GET fact + audit);
   C6↔C1 (recall/store + `get_source` for conditional provenance); C4↔C2 (ExtractFact
   consume), C4↔C1 (persist/entity/source); C7↔C2 (ReEmbed/HardDelete consume), C7↔C1
   (scans/mutate/hard_delete); C2↔C1 (store-backed table + `list_tenants`). Each is stated on both the
   producing and consuming spec. (The C6↔C5 freshness-tag and C5↔C2 ReReadSource edges are removed — C5
   retired by ADR-014.)
7. **Valid DAG.** Phase 2B + Phase 5: C1 → {C2,C3} → C4 → {C6,C7} → C8, cross-cutting at Phase 0.
   Every edge points to a higher phase; the async write path means there is no synchronous C8→C4 edge.
   No cycles.
8. **Config complete.** §2D lists every env var with Type, Default, Required, Owner, Description (46
   keys after the ADR-014/ADR-015 removals — broker, freshness, and LLM/consolidation keys gone);
   conditional/mutual-exclusion rules stated; X6 specifies load order + startup validation.
9. **Self-contained.** Each component duplicates the shared types/env/read-filter it uses into its
   Shared Context (verified: C4's `Fact` copy carries `pii_review`, and it duplicates `Source` + the
   entity/source `MemoryStore` methods it calls). A code-gen LLM can implement each from its file alone.
10. **Assumptions / open questions.** Phase 1A holds 27 LLD assumptions (SA-*) plus the inherited HLD
    assumptions; Phase 1B confirms **no question blocks spec authoring** — OQ-STORE is non-blocking for
    authoring but load-bearing for the Rust/embedded build (surfaced as the Phase 5 Step-0 precondition).
11. **Examples.** Every component carries a concrete Example (real request/response or input/output).
12. **Shared types once.** Defined in §2C, duplicated (not cross-linked) into each consuming Shared
    Context; the canonical `MemoryStore` surface lives in §2C.6 and C1 implements it exactly.
13. **Security per entry point.** Every HTTP handler (X2 auth + per-route authz in C8), every queue
    consumer (C4/C7 run post-authorisation on a job-carried scope), and the scheduler (C7 maintenance
    `ScopeContext`, structural tenant isolation) has its security posture stated; X2/X12/X9 cover the
    edge.
14. **Performance targets.** C1 (ANN ≤ 50 ms), C6 (p95 ≤ 200 ms with SA-LAT-01 sub-budgets), C3
    (token validation ≤ 5 ms warm), C8 (edge overhead ≤ 5 ms), C2 (claim p95 ≤ 20 ms). Async components
    (C4/C7) state off-read-path budgets. (The C5 ≤ 25 ms freshness sub-budget is gone — C5 retired,
    ADR-014; the read path makes no freshness check.)
15. **Locale en-GB.** All narrative prose uses en-GB spelling/idiom.
16. **Rollback surfaced.** C1 (`0001` down path, `REMOVE NAMESPACE` erasure, schemaless→schemafull
    tightening, HNSW dimension change), X7 (migration down pairs, refused destructive down on populated
    namespaces), C8 (`idempotency_record` `REMOVE TABLE`), C7 (`maintenance_state` non-destructive drop).
17. **derivedFromHld present.** All six phase files, all eight component files (incl. the retired C5
    tombstone), and the 13 cross-cutting specs carry a `derivedFromHld:` field, all now `0.6.0` after the
    RFC-01 / ADR-014 and RFC-02 / ADR-015 amendments.
18. **No hidden ADRs.** Component *Approach* sections carry only component-scope "why-this-not-that"
    (permitted by the template); no system-level Decision·Context·Consequences·Alternatives paragraph
    sits in a spec — the v1 entity-resolution trade-off was promoted to HLD `10-risks.md` + the
    `02-architecture.md` responsibility line via the Phase 3.5 HLD-impact-pass.
19. **Glossary terms in HLD.** Under ADR-014 the read-path currency terms `stale-pending-refresh` and
    `unverified-currency` were **removed** from both the HLD glossary and the spec set (no longer recall
    concepts); the `Source provenance (response)` term the LLD now uses is spec-local (an implementation
    mechanism, correctly not an HLD domain term per the bubble-up rule). Under ADR-015 the `Consolidation`
    glossary term was **reframed agent-side** in both the HLD and Phase 1 (the agent consolidates; recall
    stores), and `Consolidated Insight` retained (still a memory class the agent writes). The remaining
    Phase-1 `[LLD]` terms (`Work job`, `Dead letter`, `Trust score`, `Scope partition key`) are
    implementation-mechanism names, correctly spec-local — not HLD domain terms.
20. **Pins current.** Every `derivedFromHld:` equals the HLD's current `revision: 0.6.0`; no stale
    phase (verified by scan — zero `0.4.x`/`0.5.x` pins remain).
21. **Changelog coverage.** The HLD bumps to 0.5.0 (RFC 01 / ADR-014: passive store, agent-side
    freshness) and 0.6.0 (RFC 02 / ADR-015: LLM-free, extraction + consolidation agent-side) each have a
    matching `99-changelog.md` line. README and all child banners read 0.7.0 (D-HLD-5 sibling consistency
    holds; D-HLD-6 satisfied).

## ADR-016 re-audit (RFC 01-MCP — MCP edge + shared Service Layer, January 2026)

Scoped re-audit of the change that adds C9 (Service Layer) and C10 (MCP API Edge) and refactors C8 to
a thin adapter. Every checklist item re-verified against the changed scope:

```
[x] Every entity has a complete data model — C9/C10 own no tables (N/A stated); reused 2C types defined.
[x] Every action has inputs/outputs/numbered steps/errors — C9 (6 methods) and C10 (6 tools) each have Public Interface + numbered Internal Logic + Error Table.
[x] No banned phrases — re-scanned service-layer.md, mcp-api-edge.md, http-api-edge.md (the "handles" verbs reworded).
[x] ≥3 Gherkin per component — C9 has 4, C10 has 5.
[x] Error table ≥2 rows — C9 14 rows, C10 9 rows.
[x] Cross-component interaction on BOTH sides — C8 addendum + C10 spec both declare dependency on C9; C9 documents being consumed by both edges; C9↔C1/C2/C3/C6 ports duplicated in Shared Context.
[x] Build order is a valid DAG — C9 depends on C1/C2/C3/C6 (phase 5); C8, C10 depend only on C9 (phase 6). No cycle (phase-2 §2B DAG updated).
[x] Env vars typed/default/required/owner — MEMORY_MCP_HTTP_ADDR, MEMORY_MCP_PATH added (owner C10); idempotency + rate vars reassigned owner C8→C9.
[x] Each spec self-contained — C9/C10 duplicate ScopeContext, AppError, DTOs, config into Shared Context.
[x] Assumptions complete; OQs blocking-only — SA-SVC-01/SA-MCP-*/SA-BIN-01 added; OQ-LIB, OQ-NAME non-blocking.
[x] Example I/O — C9 and C10 each carry a concrete Example.
[x] Shared types — C9's CallContext/CallResult/RateSnapshot are C9-owned public-interface types, defined in C9 and duplicated into C10's Shared Context (consumed only by the two edges).
[x] Security for every entry point — C9 (service), C8 (HTTP), C10 (MCP) each have a Security subsection; identity is always a C3-derived ScopeContext, never request-supplied.
[x] Performance targets — C9 and C10 each carry the ≤5 ms in-process overhead budget (SA-LAT-01).
[x] en-GB throughout.
[x] Destructive schema — N/A (no DDL introduced).
[x] derivedFromHld present — C8/C9/C10 = 0.7.0; all phase + cross-cutting + component specs re-pinned to 0.7.0.
[x] No ADR-shaped paragraph in spec bodies — the Approach sections use the allowed chosen/rejected form; the full ADR-016 lives in HLD 09-decisions.md (promoted via the HLD-impact-pass).
[x] Every glossary term in the HLD glossary — MCP, Service Layer, MCP API edge added to HLD 00-overview.md; "Transport edge" is an [LLD] term (not required in HLD).
[x] derivedFromHld matches current revision — HLD now 0.7.0; all specs re-pinned to 0.7.0 (additive delta, orthogonal to C1–C7/X1–X13 content).
[x] Folder-form HLD: revision bump recorded — 99-changelog.md has the 0.7.0 line; all child banners + README read 0.7.0 (D-HLD-5 sibling consistency holds; D-HLD-6 satisfied).
```

New-component *Gaps*: `service-layer.md` — None; `mcp-api-edge.md` — None. No `[Gap]` outstanding.

## Verdict

**PASS — the spec set (including the ADR-016 additions) is *Approved*.** All Component specs have
empty *Gaps*; no Open Question is blocking (OQ-LIB / OQ-NAME are non-blocking, resolved at codegen).
`/breakdown` may consume `docs/spec/phase-5-playbook.md` — the ADR-016 work is Steps 6 (C9 → C8
refactor) and 6b (C10).

**One build-order precondition (not a spec blocker):** the OQ-STORE spike (Phase 5, Step 0) is
load-bearing for ADR-009 and must run **before** Rust scaffolding — a spike failure reopens ADR-003 and
ADR-009 and returns the work to `/design`.

**One build-order precondition (not a spec blocker):** the OQ-STORE spike (Phase 5, Step 0) is
load-bearing for ADR-009 and must run **before** Rust scaffolding — a spike failure reopens ADR-003 and
ADR-009 and returns the work to `/design`.

---

## RFC 02-consumer-tenancy-profile re-audit (January 2026, pass 1 — benchmark)

**Scope of this pass.** Acceptance criteria 1, 2, 3, 7, 8, 9 and 11 of
`docs/wip/rfc/02-consumer-tenancy-profile.md` — the measurement half. Criteria 4, 5, 6 and 10 (batched
sweep, durable cycle clock, resumable multi-tenant migration runner, optional-vector-index provisioning
variant, FU-006 resolution) are **pass 2** and are deliberately unspecified: the RFC's own handoff makes
the benchmark result the input to that decision.

**Changes audited.** Phase 1 (9 assumptions, 3 open questions, 6 glossary terms) · Phase 2 (C11 added to
2B + the DAG; 8 `MEMORY_BENCH_*` keys added to 2D) · Phase 3 (new `components/tenancy-benchmark.md`;
change note added to `components/memory-store.md`) · Phase 4 (Metrics: the per-tenant-label rejection
restated at consumer cardinality plus the per-tenant diagnosis path) · Phase 5 (Step 6c).

```
[x] Every entity has a complete data model with types and constraints.
[x] Every action has defined inputs, outputs, numbered steps, and errors.
[x] No banned phrases remain (see banned-phrases.md).
[x] Every component has >=3 Gherkin acceptance criteria (happy, edge, error).
[x] Every component has an error table with >=2 rows.
[x] Every cross-component interaction is documented on BOTH sides.
[x] Build order (Phase 2B) is a valid DAG - no circular dependencies.
[x] Every config value / env var listed with type, default, required flag, and owner component.
[x] Every spec is self-contained - implementable from its section alone.
[x] Assumptions register is complete; open questions are truly blocking only.
[x] Example I/O provided for every component with non-trivial logic.
[x] Shared types defined once in Phase 2C, referenced by name elsewhere but duplicated into "Shared Context".
[x] Security addressed for every entry point.
[x] Performance targets stated for every latency-sensitive component.
[x] All specifications use the active locale (en-GB).
[x] Destructive schema changes surface rollback considerations.
[x] Every Component, Cross-cutting, and Preamble spec carries a `derivedFromHld:` preamble field.
[x] No spec body contains an ADR-shaped paragraph.
[x] Every glossary term used in any spec is present in the HLD glossary.
[x] Every spec's `derivedFromHld:` matches the HLD's current `revision:` - no stale phases.
[x] In folder-form HLDs: every revision bump recorded in the HLD README has a corresponding line in `99-changelog.md`.
```

### Notes on ticked items

- **Cross-component interaction on both sides.** C11's single demand on C1 — `Migrator::render_up`
  widening from private to `pub(crate)` (`src/store/migrate.rs:167`) — is recorded in
  `components/tenancy-benchmark.md` (*Internal Logic* step 7, *Gaps*) and in
  `components/memory-store.md` (*Change note*). No other cross-component edge is introduced.
- **DAG validity.** C11 sits at build-order phase 7, depends on C1 only, and nothing depends on it. A
  leaf cannot introduce a cycle, and no existing step's order changes.
- **Security for every entry point.** C11 has no inbound entry point (no listener, no route, no
  subscriber), which is stated rather than left implicit. Its security surface is outbound and
  destructive and is covered by three named guards: the mandatory un-inherited `MEMORY_BENCH_STORE_URL`,
  the pre-flight prefix guard, and teardown driven by the generated id list rather than `list_tenants()`.
- **Rollback considerations.** C11 introduces no migration and no DDL into the migration set, but it
  issues `REMOVE NAMESPACE` at teardown. Because a dropped namespace has no rollback, the spec places
  the safeguards ahead of the destruction and says so explicitly.
- **Component *Gaps* sections.** All three gaps raised in the C11 initial draft were closed inside this
  pass (the `INFO FOR DB` dependency inverted so the unverifiable identifier left the critical path;
  `render_up`'s visibility verified and the widening pinned; the prefix-collision hazard closed by the
  pre-flight guard). Every component spec in the set has an empty *Gaps* section.

### HLD-impact-pass — applied

The three items above were blocked on the Phase 3.5 HLD-impact-pass, which has now been ratified and
applied. Four edits landed in `docs/design/agentic-memory/`:

1. **`00-overview.md`** — three glossary rows: `Consumer tenancy profile`, `Tenant population`,
   `Per-namespace footprint` (D-HLD-1). The other three terms this pass introduced
   (`Namespace ceiling`, `Tenant sweep`, `Namespace preamble`) stay spec-local and carry the `[LLD]`
   marker, consistent with the existing treatment of `Work job`, `Dead letter`, `Trust score`,
   `Scope partition key`, `Service Layer` and `Transport edge`.
2. **`09-decisions.md`** — the OQ-VOLUMES row now names the consumer tenancy profile as its deployment
   context and the Tenancy Benchmark Harness as the instrument that will pin it. The numbers themselves
   are **not** written: acceptance criterion 9 is satisfied by the benchmark run, not by this pass.
3. **`10-risks.md`** — one new risk: the per-operation namespace preamble is a cost that grows with
   tenant population and sits on the read path, so NFR-P2 is exposed to tenant count and not only to
   corpus size.
4. **`99-changelog.md`** — two lines (see below), and the revision propagated to the README and all
   eleven child files.

No new ADR was raised. The pass records a measurement programme and a discovered risk; it commits to no
architectural trade-off, and ADR-011 is explicitly not reopened. C11 was deliberately **not** added to
`02-architecture.md`: that diagram describes what a deployment contains, and `memory-bench` ships in no
deployment. It is recorded instead in the spec set's Phase 2B inventory under a new `tooling` type.

### Pre-existing HLD drift found and reconciled by this pass

The pass could not compute its own revision without first resolving an inconsistency it did not cause.
**ADR-017** (removed on January 2026, with its changelog line) was added to `09-decisions.md` on
January 2026 and correctly bumped **that file** to `0.8.0` — a new ADR is a minor bump — but the bump was
never propagated. Found at the start of this pass:

| Symptom | Drift label |
|---|---|
| `09-decisions.md` read `0.8.0`; README and all ten sibling child files read `0.7.0` | D-HLD-5 sibling inconsistency |
| `99-changelog.md`'s last line was January 2026 → `0.7.0`; no line for ADR-017 | D-HLD-6 changelog gap |
| Every spec pinned `derivedFromHld: 0.7.0` while an ADR had landed at `0.8.0` | D-HLD-3 stale pin |

Reconciled here: a changelog line for ADR-017 was reconstructed from the ADR's own content and is
**explicitly marked as reconstructed** in `99-changelog.md`, since the original edit recorded none. The
true base revision is therefore `0.8.0`, and this pass's patch-level edits take the HLD to **`0.8.1`**
— not `0.7.1` as the pass's own review block first proposed. All twelve HLD files and all seventeen
spec files now carry `0.8.1`.

### Verdict

**PASS.** All 21 items tick. Every Component spec has an empty *Gaps* section, no Open Question is
blocking, and the HLD and the spec set agree at revision `0.8.1`. The spec set is *Approved* for the
scope of this pass, and `/breakdown` may consume Playbook **Step 6c**.

**Scope reminder, not a defect:** Steps 6c is the measurement half only. RFC acceptance criteria 4, 5,
6 and 10 are unspecified by design, and criterion 9 (pinning OQ-VOLUMES' numbers) cannot be closed
until the benchmark has run.

**One build-order dependency:** the remote leg of Step 6c (acceptance criterion 8) needs FU-019
(`docs/wip/plan/04-remote-store-signin.md`) if the endpoint is secured. The embedded legs have no such
dependency and run first.

### Pre-existing finding, outside this pass

The Phase 3 index table in `docs/spec/phase-3-components.md` lists C1–C8 and now C11, but omits **C9
(Service Layer)** and **C10 (MCP API Edge)**, whose spec files exist at `components/service-layer.md`
and `components/mcp-api-edge.md` and are built by Playbook Steps 6 and 6b. This predates the present
pass (it dates from the ADR-016 amendment of January 2026) and is left uncorrected here rather than fixed
opportunistically. It is a two-row index fix whenever the ADR-016 work is next touched.

---

## RFC 04 re-audit (January 2026 — write authority by fact class, ADR-019)

**Scope of this pass.** All eleven acceptance criteria of
`docs/wip/rfc/04-write-authority-by-fact-class.md`. The three blocking decisions were settled with the
requester on January 2026 and are recorded in the RFC: the class label is a first-class `fact_class`
request field; authority is carried in a strictly-read `memory_fact_authority` token claim with no
stored table; an absent claim grants every class, with the absence reported at start-up and on a
counter. Open questions 4 to 7 were non-blocking and are settled by this pass — see *Questions settled*
below.

**Changes audited.** Phase 1 (3 assumptions: SA-AUTHZ-01, -02, -07) · Phase 2 (2C.3 gains `client`,
`fact_authority` and the `FactAuthority` type plus the binding authority rule; 2C.4 gains
`RememberRequest.fact_class`; 2D gains `MEMORY_OIDC_CLIENT_CLAIM` and `MEMORY_OIDC_AUTHORITY_CLAIM`) ·
Phase 3 (`auth-scope.md` claim extraction and strict parsing; `service-layer.md` the enforcement point;
`http-api-edge.md` and `mcp-api-edge.md` the request field and the new 403; `memory-store.md` the two
audit columns and migration `0005`; index corrected to list C9 and C10) · Phase 4 (X1 registry gains
`AUTHZ_CLASS_FORBIDDEN`; X2 gains the write-authority policy; X4 gains the three counters) · Phase 5
(Step 6d).

```
[x] Every entity has a complete data model with types and constraints.
[x] Every action has defined inputs, outputs, numbered steps, and errors.
[x] No banned phrases remain (see banned-phrases.md).
[x] Every component has >=3 Gherkin acceptance criteria (happy, edge, error).
[x] Every component has an error table with >=2 rows.
[x] Every cross-component interaction is documented on BOTH sides.
[x] Build order (Phase 2B) is a valid DAG - no circular dependencies.
[x] Every config value / env var listed with type, default, required flag, and owner component.
[x] Every spec is self-contained - implementable from its section alone.
[x] Assumptions register is complete; open questions are truly blocking only.
[x] Example I/O provided for every component with non-trivial logic.
[x] Shared types defined once in Phase 2C, referenced by name elsewhere but duplicated into "Shared Context".
[x] Security addressed for every entry point.
[x] Performance targets stated for every latency-sensitive component.
[x] All specifications use the active locale (en-GB).
[x] Destructive schema changes surface rollback considerations.
[x] Every Component, Cross-cutting, and Preamble spec carries a `derivedFromHld:` preamble field.
[x] No spec body contains an ADR-shaped paragraph.
[x] Every glossary term used in any spec is present in the HLD glossary.
[x] Every spec's `derivedFromHld:` matches the HLD's current `revision:` - no stale phases.
[x] In folder-form HLDs: every revision bump recorded in the HLD README has a corresponding line in `99-changelog.md`.
```

### Questions settled by this pass

| # | Question | Answer | Where it is recorded |
|---|---|---|---|
| 4 | Does class authority imply authority to retire or hard-delete? | **No.** Class authority governs assertion only. Retire and delete stay governed by `Op::Forget` plus C1's ownership check. Deriving a class from the *stored* record would require reading it before authorising, which turns a `404 NOT_FOUND` into a signal that the record exists and would break acceptance criterion 3. | SA-AUTHZ-07 (Phase 1A), `service-layer.md` §Security + Gherkin, X2 policy |
| 5 | Does source binding ship with the class rule or after it? | **With it.** The claim entry already carries the optional `source`, the enforcement point is the same comparison, and acceptance criterion 4 asserts it. An entry with `source: None` admits any `origin_ref`; an entry with `source: Some(s)` admits only an exact match and refuses a request carrying no source. | SA-AUTHZ-05, `service-layer.md` Internal Logic step 5a |
| 6 | Does a refused write consume rate-limit budget? | **Yes, deliberately.** The bucket is decremented at step 3, before the class check at step 5a. A principal probing which classes it may assert pays for each attempt. The counterpart is that a refusal consumes no idempotency key, because the outcome is stored only after the component call succeeds. | `service-layer.md` Internal Logic step 5a, Gherkin |
| 7 | A new error code, or reuse `SCOPE_FORBIDDEN`? | **A new code, `AUTHZ_CLASS_FORBIDDEN` (403).** `SCOPE_FORBIDDEN` means *you do not own that record*; the new code means *your token may not assert that class*. The first is diagnosed by looking at the record, the second by looking at the token, and SA-ENV-02's rule that each code carries exactly one meaning settles it the same way. The caller-facing message stays generic. | X1 registry, `service-layer.md` error table, `http-api-edge.md`, `mcp-api-edge.md` |

### Notes on ticked items

- **Cross-component interaction on both sides.** Four edges are introduced and each is written on both
  ends: C3 → C9 (the two new `ScopeContext` fields, in `auth-scope.md` §Public Interface and
  `service-layer.md` §Shared Context); C9 → C1 (`AuditEntry.client` / `.fact_class`, in
  `service-layer.md` step 6 and `memory-store.md` §Data Model + `append_audit`); C8 → C9 and C10 → C9
  (the `fact_class` request field, in each edge's route/tool description and in C9's Internal Logic).
- **Destructive schema changes surface rollback considerations.** Migration `0005_audit_authority` is
  additive — two `DEFINE FIELD IF NOT EXISTS … TYPE option<string>` on `audit_log`. It backfills
  nothing and collapses no bi-temporal history. Its *down* path is destructive in a way worth naming
  and is named in `memory-store.md`: `REMOVE FIELD` discards the record of which service acted and
  which class was refused, which is precisely the evidence the control exists to produce.
- **Every glossary term used in any spec is present in the HLD glossary.** `fact class` and
  `write authority` were added to `00-overview.md` by this pass's HLD-impact-pass (D-HLD-1).
- **No spec body contains an ADR-shaped paragraph.** The trade-offs — claim versus table, open versus
  closed default, field versus convention — are stated once as ADR-019 in `09-decisions.md`. The specs
  state the resulting rule and the reason it sits where it sits, which is component-level reasoning,
  not an alternatives-considered block.
- **Security addressed for every entry point.** Both edges are covered by one check, which is the
  point: `SA-SVC-01` already required one definition of auth/scope/rate/idempotency/audit, and this
  joins that set rather than sitting beside it. Acceptance criterion 6 is asserted against both
  surfaces rather than inferred from the shared service layer.

### HLD-impact-pass — applied

| Finding | Target | Edit |
|---|---|---|
| New ADR-shaped trade-off (claim-carried authority, open default, strict parsing) | `09-decisions.md` | **ADR-019** added |
| New cross-cutting policy applying across components | `07-cross-cutting.md` | *Write authority (fact class)* row added |
| New domain terms | `00-overview.md` | Glossary rows *Fact class* and *Write authority* added |
| Revision bump + changelog | all HLD files, `99-changelog.md` | `0.9.0 → 0.10.0`, one changelog line |

All 16 spec files re-pinned to `derivedFromHld: 0.10.0`.

### Pre-existing finding, now closed

The Phase 3 index omitted **C9 (Service Layer)** and **C10 (MCP API Edge)** from the ADR-016 amendment
of January 2026 onwards. The January 2026 audit recorded it and deferred it to whenever the ADR-016 work
was next touched. This pass touches both components, so the two rows are added and C8's dependency
column is corrected from `C1, C2, C3, C6` to `C9` — after ADR-016 the HTTP edge is a thin adapter over
the Service Layer.

### Verdict

**PASS.** All 21 items tick. Every Component spec has an empty *Gaps* section, no Open Question is
blocking, and the HLD and the spec set agree at revision `0.10.0`. The spec set is *Approved* for the
scope of this pass, and `/breakdown` may consume Playbook **Step 6d**.

**Stated limitation, not a defect.** The control governs *labelled* assertions. A caller that omits
`fact_class` asserts no class and is admitted on the write scope alone. Closing that would require
inferring a class from content, which ADR-015 and `SA-LLMFREE-01` both forbid and which the RFC places
out of scope. This is recorded in SA-AUTHZ-01's *Impact if wrong* column and in ADR-019's consequences,
and it follows from the settled decisions rather than being open for choice.

**Finding raised by implementation — RFC 04 open question 8, outside this pass's scope.** The
authorised label is **discarded after the decision**. `remember_payload` carries `content`,
`memory_class` and `source` into the enqueued job and nothing else, and `Fact` has no class field, so
nothing persists `fact_class`. Two consequences follow. A consumer cannot read back the label that was
authorised — it reads whatever the caller put inside `content`, which `memory` never compared against
`fact_class`. And a caller may therefore send a class it *is* entitled to assert in `fact_class` while
labelling the same write differently inside `content`, pass the gate, and produce a fact its own
readers treat as the protected class. All eleven of RFC 04's acceptance criteria are met without
storing the class — none of them mentions persistence — so this pass is not failed by it. But the
control is materially more useful with the class stored, and that is a decision with its own blast
radius (`Fact`, the write pipeline, a migration, the read path) rather than an implementation detail
this pass should have absorbed. It is recorded as RFC 04 open question 8 for a follow-up pass.

**One cross-repository dependency.** Acceptance criterion 11 requires the Companion Platform's
`MP-CONV-2` to pass against a container built from this repository, and its canary assertion to be
inverted from recording a `202` to requiring a `403`. That is a change in the sibling's corpus, not
this one, and cannot be closed here.

---

## RFC 05 re-audit (January 2026 — storing the asserted fact class)

**Scope of this pass.** All ten acceptance criteria of
`docs/wip/rfc/05-storing-the-asserted-fact-class.md`. RFC 05 exists because RFC 04's implementation
authorised a class and then discarded it; this pass makes the authorised label the stored label. Both
of RFC 05's own open questions are non-blocking and are settled below.

**Changes audited.** Phase 1 (2 assumptions: SA-AUTHZ-09, SA-AUTHZ-10) · Phase 2 (2C.2 `Fact` gains
`fact_class`) · Phase 3 (`service-layer.md` carries it into the job payload; `write-pipeline.md`
declares it on `JobPayload`, persists it on the fact, carries it onto `QuarantineRecord`, and adds
migration `0006`; `memory-store.md` duplicates the field and records why `fact` needs no migration;
`http-api-edge.md` and `mcp-api-edge.md` state that every `Fact`-bearing response carries it) ·
Phase 4 (unchanged — no new error code, no new cross-cutting policy) · Phase 5 (Step 6e).

```
[x] Every entity has a complete data model with types and constraints.
[x] Every action has defined inputs, outputs, numbered steps, and errors.
[x] No banned phrases remain (see banned-phrases.md).
[x] Every component has >=3 Gherkin acceptance criteria (happy, edge, error).
[x] Every component has an error table with >=2 rows.
[x] Every cross-component interaction is documented on BOTH sides.
[x] Build order (Phase 2B) is a valid DAG - no circular dependencies.
[x] Every config value / env var listed with type, default, required flag, and owner component.
[x] Every spec is self-contained - implementable from its section alone.
[x] Assumptions register is complete; open questions are truly blocking only.
[x] Example I/O provided for every component with non-trivial logic.
[x] Shared types defined once in Phase 2C, referenced by name elsewhere but duplicated into "Shared Context".
[x] Security addressed for every entry point.
[x] Performance targets stated for every latency-sensitive component.
[x] All specifications use the active locale (en-GB).
[x] Destructive schema changes surface rollback considerations.
[x] Every Component, Cross-cutting, and Preamble spec carries a `derivedFromHld:` preamble field.
[x] No spec body contains an ADR-shaped paragraph.
[x] Every glossary term used in any spec is present in the HLD glossary.
[x] Every spec's `derivedFromHld:` matches the HLD's current `revision:` - no stale phases.
[x] In folder-form HLDs: every revision bump recorded in the HLD README has a corresponding line in `99-changelog.md`.
```

### Correction made during this pass

RFC 05's lane rationale first recorded the schema signal as **negative**, on the strength of `fact`
being `SCHEMALESS` (`migrations/0001_init.up.surql:27`). That was checked against the wrong table.
`quarantine` is `SCHEMAFULL` (`migrations/0001_init.up.surql:131`) and rejects a write carrying an
undeclared key outright — so a quarantined write carrying a class would have failed *entirely*, losing
the held record rather than just the label. Migration `0006_quarantine_fact_class` is required and is
specified in `components/write-pipeline.md`. The RFC's rationale, constraints and proposed direction
were corrected rather than left to be discovered at implementation time; acceptance criterion 9 was
added to assert the migration applies to a namespace already at version 5.

`fact` still needs no migration, and `memory-store.md` says so explicitly, so a reader does not go
looking for one that deliberately does not exist.

### Questions settled by this pass

| # | Question | Answer |
|---|---|---|
| 1 | How does `fact_class` appear in the OpenAPI `Fact` schema? | An optional string with no enum, described the way `memory_class` is described but explicitly *not* enumerated — the vocabulary is the caller's and `memory` validates it against nothing. `skip_serializing_if = "Option::is_none"` so an unclassed fact omits the key rather than emitting `null` (RFC 05 acceptance criterion 3). |
| 2 | Does C7's re-embed or decay pass preserve the class across a rewrite? | Yes by construction, and asserted rather than assumed: C7's duties (`set_fact_embedding`, decay updates, supersession) mutate named columns on an existing row and never rewrite the whole record, so an untouched key survives. A test in Step 6e covers a decayed and a re-embedded fact retaining its class. |

### Notes on ticked items

- **Cross-component interaction on both sides.** Three edges, each written on both ends: C9 → C4 (the
  payload key, in `service-layer.md` step 5 and `write-pipeline.md` step 0); C4 → C1 (`Fact.fact_class`
  persisted, in `write-pipeline.md` step 8 and `memory-store.md` §Data Model); C1 → C8/C10 (the field
  on every `Fact`-bearing response, in `memory-store.md` and both edge specs).
- **Destructive schema changes surface rollback considerations.** `0006_quarantine_fact_class` is
  additive. Its down path removes the column, discarding the record of what each held write claimed to
  be while the quarantine rows themselves survive — stated in `memory-store.md` alongside `0005`'s.
- **Every glossary term used in any spec is present in the HLD glossary.** `fact class` was added by
  the RFC 04 pass; this pass adds it to the Fact entity's key fields in `04-domain.md`, which is where
  a reader of the domain model would look for it.
- **No spec body contains an ADR-shaped paragraph.** The trade-off — store versus filter — is recorded
  once, as the amendment to ADR-019's consequences. The specs state the resulting rule.
- **Performance targets stated.** The read path is unchanged: no index, no new predicate, one extra
  string on a row already being read. `RecallFilters` is untouched, which is the assertion that keeps
  this off the NFR-P2 budget.

### HLD-impact-pass — applied

| Finding | Target | Edit |
|---|---|---|
| Material change to an existing ADR's consequences | `09-decisions.md` | ADR-019 amended: the authorised class is stored and returned, not merely compared |
| New key field on a domain entity | `04-domain.md` | *fact class* added to the Fact entity's key fields |
| Revision bump + changelog | all HLD files, `99-changelog.md` | `0.10.0 → 0.10.1` (patch: an existing decision made observable, no new decision), one changelog line |

A patch bump rather than a minor one: RFC 05 reverses nothing and introduces no new decision. It
completes ADR-019 by making its judgement legible to a reader. All 18 spec files re-pinned to
`derivedFromHld: 0.10.1`.

### Verdict

**PASS.** All 21 items tick. Every Component spec has an empty *Gaps* section, no Open Question is
blocking, and the HLD and the spec set agree at revision `0.10.1`. The spec set is *Approved* for the
scope of this pass, and `/breakdown` may consume Playbook **Step 6e**.

**What this closes.** RFC 04 open question 8 is answered: the label the write gate authorises is the
label stored and returned, so a consumer no longer has to trust a class inside `content` that `recall`
compared against nothing. The residual limit stated in the RFC 04 audit still stands and is unchanged
by this pass — the control governs *labelled* assertions, and an unclassed write asserts no class.

**Deliberately not done.** The class is not filterable. Adding it to `RecallFilters` would need an
index on the fact table and a stage-1 query predicate against the NFR-P2 read budget, for a capability
no consumer has asked for, when the field is already in the response for a consumer to filter on. If
one later measures a corpus size where client-side filtering stops working, that is its own change.

## ADR-020 re-audit (January 2026 — collections, rank fusion and typed sub-queries, ADR-020)

**Scope of this pass.** The `memory/` change M-8 that ordin's ADR-110 (decision 90) needs, as consumed by `docs/spec/ordin-desktop/components/MemoryService.md` (amendment January 2026). Which candidates survive the stage-1 cap is settled in `memory-store.md`: the as-built cap keeps the first `stage1_k` facts in fact-id order (`src/store/mod.rs:716`), which was not intended, and step 10′ replaces it. Open question 7 (fusion targets candidate selection and the degraded path, not the reranked order) is the shape of the C6 amendment.

**Changes audited.** Phase 1 (assumptions SA-COLL-01–05, SA-SUBQ-01–03, SA-SVC-CHECK-01, SA-BENCH-COLL-01–02; glossary Collection, Reciprocal rank fusion, Typed sub-query) · Phase 2 (2C.2 `collection` on `Fact`, `Entity`, `Relationship`, `Source`; 2C.4 `RecallRequest.searches`, `SubQuery`, `SubQueryKind`, `RecallFilters.collections`, `RememberRequest.collection`, `CheckContentRequest`, `ContentCheck`, `PersonalDataFinding`; 2C.6 `CollectionErasure`; 2D three `MEMORY_BENCH_*` rows) · Phase 3 (amendments January 2026 to `memory-store.md`, `retrieval-engine.md`, `service-layer.md`, `write-pipeline.md`, `http-api-edge.md`, `mcp-api-edge.md`, `tenancy-benchmark.md`) · Phase 4 (registry: C9 added as a producer of `VAL_INVALID_BODY` and `VAL_OUT_OF_RANGE`; *Retryable* column added) · Phase 5 (Step 6f).

```
[x] Every entity has a complete data model with types and constraints.
[x] Every action has defined inputs, outputs, numbered steps, and errors.
[x] No banned phrases remain (see banned-phrases.md).
[x] Every component has >=3 Gherkin acceptance criteria (happy, edge, error).
[x] Every component has an error table with >=2 rows.
[x] Every client-visible error code the SOURCE emits appears in the cross-cutting error-code table and in its retryability table.
[x] Every cross-component interaction is documented on BOTH sides.
[x] Build order (Phase 2B) is a valid DAG - no circular dependencies.
[x] Every config value / env var listed with type, default, required flag, and owner component.
[x] Every spec is self-contained - implementable from its section alone.
[x] Assumptions register is complete; open questions are truly blocking only.
[x] Example I/O provided for every component with non-trivial logic.
[x] Shared types defined once in Phase 2C, referenced by name elsewhere but duplicated into "Shared Context".
[x] Security addressed for every entry point.
[x] Performance targets stated for every latency-sensitive component.
[x] All specifications use the active locale (en-GB).
[x] Destructive schema changes surface rollback considerations.
[x] Every Component, Cross-cutting, and Preamble spec carries a `derivedFromHld:` preamble field.
[x] No spec body contains an ADR-shaped paragraph.
[x] Every glossary term used in any spec is present in the HLD glossary.
[x] Every spec's `derivedFromHld:` matches the HLD's current `revision:` - no stale phases.
[x] In folder-form HLDs: every revision bump recorded in the HLD README has a corresponding line in `99-changelog.md`.
```

### Notes on ticked items

- **Error codes, enumerated from the source.** The codes `src/` emits today are `AUTHZ_CLASS_FORBIDDEN`, `AUTH_INSUFFICIENT_SCOPE`, `AUTH_INVALID_TOKEN`, `AUTH_MISSING_TOKEN`, `INTERNAL`, `NOT_FOUND`, `PROVIDER_ERROR`, `PROVIDER_TIMEOUT`, `QUEUE_UNAVAILABLE`, `RATE_LIMITED`, `SCOPE_FORBIDDEN`, `STORE_TIMEOUT`, `STORE_UNAVAILABLE`, `VAL_BODY_TOO_LARGE`, `VAL_INVALID_BODY`, `VAL_MISSING_IDEMPOTENCY_KEY`, `VAL_OUT_OF_RANGE`, `VAL_UNSUPPORTED_CLASS`; each has a registry row. The amendments add no code. **Corrected during this pass:** the registry had no retryability information — the item requires it, and the checklist item post-dates the RFC 05 audit — so a *Retryable* column was added; and C9, which has produced `VAL_INVALID_BODY` since RFC 04 (`fact_class`), was missing from that row's producers.
- **Cross-component interaction on both sides.** C9 → C4 (`remember.collection` in the payload: `service-layer.md`, `write-pipeline.md`); C4 → C1 (collection persisted, `quarantine.collection`: `write-pipeline.md`, `memory-store.md`); C6 → C1 (`StageOneQuery.searches`, `Candidate.fused_*`: `retrieval-engine.md`, `memory-store.md`); C8 / C10 → C9 (schemas: both edges, `service-layer.md`); C11 → C1 (collection leg: `tenancy-benchmark.md`, `memory-store.md` performance); ordin → C9 / C1 (`check_content`, `CollectionErasure`: `service-layer.md`, `memory-store.md`, and ordin's `MemoryService.md` M-8).
- **Build order.** C11 still depends on C1 only: the collection leg measures stage-1 and the fused order, and the reranked order and whole-path p95 with models are measured by ordin's Memory Service performance scenario. No edge added.
- **Destructive schema changes.** Migration `0007_fact_collection` is additive (a field with a default, a backfill of `''` and an index on four tables, plus `quarantine.collection`); its down path is stated in `memory-store.md` and `write-pipeline.md`. `erase_collection` is a verified predicate delete with a proof, guarded so an entity or source another collection still names is kept.
- **ADR-shaped paragraphs.** The trade-off (rank versus raw score, filter-first versus global top-K, typed sub-queries versus model rewording) is ADR-020; each spec's *Approach* states the rule and names its rejected alternatives in one line, as every earlier amendment does.
- **Pins current.** HLD revision `0.11.0`; all spec files re-pinned to `derivedFromHld: 0.11.0`; one changelog line.

### HLD-impact-pass — applied

| Finding | Target | Edit |
|---|---|---|
| New ADR-shaped trade-off | `09-decisions.md` | ADR-020 |
| New glossary terms | `00-overview.md` | Collection, Reciprocal rank fusion, Typed sub-query |
| New key field on a domain entity | `04-domain.md` | *collection* on the Fact entity (and the entities, relationships and sources a write creates) |
| New externally visible shapes | `08-interfaces.md` | Collections and typed sub-queries section (recall and remember bodies, `collection` on returned records, the in-process content check and collection erasure) |
| System-level risks | `10-risks.md` | Stage-1 cap kept the oldest candidates (closed by ADR-020); per-collection stage-1 cost |
| Revision bump + changelog | all HLD files, `99-changelog.md` | `0.10.1 → 0.11.0` (minor: a new ADR) |

### Verdict

**PASS** for the scope of this pass: every item ticks, every amended Component spec has an empty *Gaps* section, and no Open Question is blocking. The user promoted the amendments to *Approved* on January 2026; `/breakdown` may consume Playbook **Step 6f**. Measurements ordin's ADR-110 runs read against (fairness, exact match, filtered recall and the stage-1 share of the budget run) are produced by the C11 collection leg; their thresholds are ordin's (ADR-110), and the report carries no verdict.

## ADR-021 re-audit (January 2026 — entities, links and graph recall, ADR-021)

**Scope of this pass.** The `memory/` change M-9 that ordin's ADR-111 (decision 91) needs, as consumed by `docs/spec/ordin-desktop/components/MemoryService.md` (amendment January 2026). Two as-built facts were found and recorded rather than changed: the decay time unit is seconds (`MEMORY_DECAY_K = 10` gives a 10-second scale) and the salience heuristic keeps every written fact above the floor, so decay never prunes (`maintenance-worker.md` amendment; HLD `10-risks.md`). The component has no consolidation of its own to switch off (ADR-015 stands). Forgetting a fact still removes the summaries derived from it (SA-DELETE-01, unchanged).

**Changes audited.** Phase 1 (assumptions SA-GRAPH-01–12, SA-BENCH-GRAPH-01; glossary Link, Entity match, Link expansion, Fading, Keep) · Phase 2 (2C.2 `Relationship.replaces` and the persistence-only `entity.names_norm` and `relationship_fact`; 2C.4 `RememberRequest.entities` / `links` / `derived_from`, `EntityAssertion`, `LinkAssertion`, `RankedFact.fading`, `RecallResponse.links`, `RecallLink`, the request/result types of the ten new operations; 2C.6 `GraphStore`, `LinkRole`, `Neighbour`, `EntitySummary`, `EntityPage`, `EntityDetail`, `EntityCount`, `EntityErasureProof`; 2D `MEMORY_GRAPH_HOPS`, `MEMORY_GRAPH_FANOUT`, `MEMORY_GRAPH_CANDIDATE_CAP`, `MEMORY_TRUST_WEIGHT`, `MEMORY_BENCH_GRAPH_SHAPE`, and a note on `MEMORY_DECAY_K`) · Phase 3 (amendments January 2026 to `memory-store.md`, `write-pipeline.md`, `retrieval-engine.md`, `maintenance-worker.md`, `service-layer.md`, `http-api-edge.md`, `mcp-api-edge.md`, `tenancy-benchmark.md`) · Phase 4 (registry: C9 added as a producer of `NOT_FOUND`) · Phase 5 (Step 6g).

```
[x] Every entity has a complete data model with types and constraints.
[x] Every action has defined inputs, outputs, numbered steps, and errors.
[x] No banned phrases remain (see banned-phrases.md).
[x] Every component has >=3 Gherkin acceptance criteria (happy, edge, error).
[x] Every component has an error table with >=2 rows.
[x] Every client-visible error code the SOURCE emits appears in the cross-cutting error-code table and in its retryability table.
[x] Every cross-component interaction is documented on BOTH sides.
[x] Build order (Phase 2B) is a valid DAG - no circular dependencies.
[x] Every config value / env var listed with type, default, required flag, and owner component.
[x] Every spec is self-contained - implementable from its section alone.
[x] Assumptions register is complete; open questions are truly blocking only.
[x] Example I/O provided for every component with non-trivial logic.
[x] Shared types defined once in Phase 2C, referenced by name elsewhere but duplicated into "Shared Context".
[x] Security addressed for every entry point.
[x] Performance targets stated for every latency-sensitive component.
[x] All specifications use the active locale (en-GB).
[x] Destructive schema changes surface rollback considerations.
[x] Every Component, Cross-cutting, and Preamble spec carries a `derivedFromHld:` preamble field.
[x] No spec body contains an ADR-shaped paragraph.
[x] Every glossary term used in any spec is present in the HLD glossary.
[x] Every spec's `derivedFromHld:` matches the HLD's current `revision:` - no stale phases.
[x] In folder-form HLDs: every revision bump recorded in the HLD README has a corresponding line in `99-changelog.md`.
```

### Notes on ticked items

- **Error codes.** The amendments add no code; every new failure maps to `VAL_INVALID_BODY`, `VAL_OUT_OF_RANGE`, `NOT_FOUND` or `INTERNAL`. **Corrected during this pass:** C9 was missing from `NOT_FOUND`'s producers (it already returned it for `get_fact`), and now returns it from the new operations.
- **Cross-component interaction on both sides.** C9 → C4 (entities, links, `derived_from` in the payload: `service-layer.md`, `write-pipeline.md`); C4 → C1 (`GraphStore` writes, the closing rule: `write-pipeline.md`, `memory-store.md`); C6 → C1 (`StageOneQuery.graph_*`, `Candidate.source_trust` / `graph_hop` / `via`: `retrieval-engine.md`, `memory-store.md`); C6 → C7 (`is_fading`: `retrieval-engine.md`, `maintenance-worker.md`); C7 → C1 (`backfill_entity_names`: both); C8 / C10 → C9 (routes and schemas: both edges, `service-layer.md`); C11 → C1 (graph leg: `tenancy-benchmark.md`, `memory-store.md` performance); ordin → C9 / C1 (person operations, team import/export carrying links, SQL storage of `GraphStore`: `service-layer.md`, `memory-store.md`, and ordin's `MemoryService.md` M-9).
- **Build order.** C7's new call (`backfill_entity_names`) is on C1, which it already depends on. C6 calls C7's pure functions (`reinforce` before this change, `is_fading` now); the edge C6 → C7 was not in 2B and is now recorded there. C7 does not depend on C6, so the graph stays a DAG; both are built in Step 5, C7's pure functions first.
- **Destructive schema changes.** Migration `0008_graph` is additive on the way up. Its down path removes the `relationship_fact` table, which loses the record of which facts asserted each link; the rollback note in `memory-store.md` states the consequence and the restore route.
- **ADR-shaped paragraphs.** The trade-offs (graph as rank-fused lists, exact entity matching, links on the fact's own job, in-process-only person operations) are ADR-021; each spec's *Approach* states the rule and names its rejected alternatives in one line.
- **Pins current.** HLD revision `0.12.0`; all spec files re-pinned to `derivedFromHld: 0.12.0`; one changelog line.

### HLD-impact-pass — applied

| Finding | Target | Edit |
|---|---|---|
| New ADR-shaped trade-off | `09-decisions.md` | ADR-021 |
| New glossary terms | `00-overview.md` | Link, Entity match, Link expansion, Fading, Keep |
| New key fields and lifecycle rules | `04-domain.md` | Entity: normalised names; Relationship: asserting facts, *replaces*, closing and erasure rules |
| New externally visible shapes | `08-interfaces.md` | Entities, links and graph recall section (remember and recall bodies, four read routes, in-process person operations) |
| System-level risks | `10-risks.md` | Decay never prunes and its seconds unit (as-built); link-expansion cost; forget reaches summaries |
| Revision bump + changelog | all HLD files, `99-changelog.md` | `0.11.0 → 0.12.0` (minor: a new ADR) |

### Verdict

**PASS** for the scope of this pass: every item ticks, every amended Component spec has an empty *Gaps* section, and no Open Question is blocking. The amendments stay *Draft* until the user promotes them; once promoted, `/breakdown` may consume Playbook **Step 6g**. The numbers ordin's ADR-111 runs read against (criterion 5 budget, criteria 3 and 4 fixtures) are produced by the C11 graph leg; their thresholds are ordin's, and the report carries no verdict.
