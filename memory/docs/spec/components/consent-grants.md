### SPEC: Consent Grants (C12)

**File:** `src/types/grant.rs` (types) · `migrations/0004_consent_grants.up.surql` (table and indexes) · `src/store/mod.rs` (the read-filter arm) · `src/grants` **not yet created** (the write half) | **Package:** `memory::types::grant`, `memory::store` | **Phase:** 2 | **Dependencies:** Auth & Scope (`ScopeContext`), Memory Store (`grant` table and the read-filter fragment)

> **Mode:** greenfield
> **derivedFromHld:** 0.12.0
> **As built (January 2026):** this component is **half-built, and the halves live in different places
> from the single module this spec originally named.** The read half is live: `Grant`,
> `GrantOperation`, `IssueGrant`, `GrantSide` and `GrantQuery` in `src/types/grant.rs`; the `grant`
> table with `grant_grantee_active (grantee, revoked_at, valid_to)` and `grant_fact` in
> `migrations/0004_consent_grants.up.surql`; and the fourth arm inside `fact_filter_clause`
> (`src/store/mod.rs`), whose text is reachable from tests via `grant_arm_sql()`. The write half —
> `issue`, `revoke`, `list` and any transport exposure — **does not exist**: there is no `src/grants`
> module, and no service-layer or edge surface references a grant. Rows reach the table today only
> through `Store::put_grant_for_test` / `revoke_grant_for_test`, which are `pub(crate)` and named for
> what they are. Those two exist because the arm shipped inert for want of a test that put a row in
> the table and asked whether the grantee could read it (FU-026, since fixed: `grant.fact` is a
> string and `fact.id` a record link, so the bare comparison matched nothing and every test over the
> fragment *text* still passed). The `// src/grants/mod.rs` block under *Public Interface* is
> therefore a specification of work not yet done, and is labelled there as such.
> **Change origin:** OQ-C1 of `companion/docs/spec/companion-platform/phase-1-analysis.md`. The consuming design places a directional per-fact consent predicate inside this service's query; no such model existed.

#### Purpose

Consent Grants adds a second, narrower way for a caller to read a fact: a **directional, per-fact, expiring grant** issued by the fact's owner to one named principal for one stated purpose. It exists because the existing read filter is a *scope* model — it answers "is this caller inside the owning tenant, user or team?" — and cannot express "this specific person may read this specific fact, for this reason, until this date, and I may withdraw it at any moment."

The grant is evaluated **inside the store query**, as a fourth arm of the read filter, so a fact the caller has no grant for is never fetched and then discarded. Revocation is therefore immediate by construction: there is no cache to go stale and no second round trip in which a revoked grant could still be honoured.

A grant widens a read. It never widens a write, never confers a scope, and never cascades — holding a grant over a fact conveys nothing about any other fact, including facts derived from it.

#### Approach

The grant is a first-class record in its own `grant` table rather than a `Fact` with a special class, for one reason: it must be **queryable structure**, because the read filter joins against it. A grant encoded inside a fact's opaque `content` JSON could not be indexed and could not appear in a `WHERE` fragment, which would force the filter back out of the query and onto the request path — the exact placement the consuming design's ADR-009 refuses.

**The grant arm is excluded from every stage-1 search signal**, and this is the load-bearing property of the component. It was reasoned as a vector-path decision and turned out to be broader; the reasoning is recorded first, then the correction. `src/store/mod.rs` selects between two vector strategies by `MemoryProfile`:

- `PostFilteredAnn` walks the HNSW index for the K nearest neighbours and then applies the read filter. Its own source note records that it is wrong at a low visible fraction, because a visible row can be crowded out of the candidate window by rows the caller cannot read.
- `FilterFirstExact` filters first and scores every survivor, driven by a `WITH INDEX fact_owner_user` hint that its source note records as "only sound because `read` is the narrow owner equality".

**A grant-scoped read is by construction a low-visible-fraction read** — a grantee typically holds grants over a handful of facts among many thousands. Under `PostFilteredAnn` a granted fact would routinely fail to surface at all. Under `FilterFirstExact` the single-column index hint would be unsound, because a grant arm is not an owner equality. Neither strategy is correct for it, so vector search over granted facts is **not implemented** and is rejected at the boundary rather than returning a silently incomplete result. Grant-scoped reads are served by direct fetch, keyword search and entity traversal, all of which apply the filter without an approximate candidate window.

**The correction (RISK-020).** The reasoning above concerns the two vector strategies and is sound for
both. It is not the whole picture. `memory` applies `vector_read_filter` to **all three** stage-1
signals — vector, keyword and graph — deliberately, so that "a fact the keyword leg could return but
the vector leg could not" cannot make relevance depend on which signal happened to match
(`memory/src/store/mod.rs`, RISK-017). Withholding the grant arm from that one function therefore
silences it across every search path, not only the approximate one.

The net behaviour today should be read plainly: **a granted fact is fetchable by id and is not findable
by any search.** That is a coherent position — a grant confers id-scoped access — but it is a product
answer rather than an implementation detail, and it is **not yet decided**. It is recorded as an open
question below rather than asserted as a design.

The three deliberately rejected alternatives: **(1)** a grant checked in the service layer after retrieval — rejected because it fetches what it then hides, and the fetch is the disclosure; **(2)** a grant expressed as a new `Visibility` variant — rejected because visibility is a property of the record and a grant is a relation between a record and a principal, so one grantee's access would widen every grantee's; **(3)** a grant that cascades to derived facts — rejected because a consolidated insight can be derived from facts the grantee holds no grant over, so cascading would leak the inputs through the output.

#### Shared Context

Duplicated verbatim from Phase 2 so this spec is implementable from this section alone.

```rust
// src/types/scope.rs — existing, unchanged.
pub struct ScopeContext {
    pub tenant: String,
    pub teams: Vec<String>,
    pub user: String,
    pub token_jti: String,
    pub allowed_ops: OpSet,
    pub correlation_id: String,
}
```

**Existing read-filter rule (Phase 2C.3), which this component extends rather than replaces:** a caller may read a Fact iff `record.owner.tenant == ctx.tenant` **and** ( `record.owner.user == ctx.user` **or** (`record.visibility == TeamShared` and `record.owner.team ∈ ctx.teams`) **or** `record.visibility == TenantShared` ).

#### Public Interface

**Built**, in `src/types/grant.rs`. The module also carries `IssueGrant`, `GrantSide` and
`GrantQuery`, which the write half's signatures below consume.

```rust
// src/types/grant.rs

/// A directional permission from one fact's owner to one named principal.
/// Never inferred from a relationship, a team, a role or any other grant.
#[derive(Serialize, Deserialize, Clone)]
pub struct Grant {
    /// "grant:<uuidv7>" (SA-ID-01).
    pub id: String,
    /// The fact this grant permits reading. Exactly one; a grant is never a set.
    pub fact: String,
    /// The scope that issued it. Must equal the fact's `owner` at issue time.
    pub grantor: ScopeRef,
    /// The user id permitted to read. Matched against `ScopeContext.user`.
    /// Never a team, never a tenant, never a wildcard.
    pub grantee: String,
    /// Why the grantee may read it, recorded for audit and shown to the grantor
    /// on review. Free text, 1..=512 chars, never interpreted by recall.
    pub purpose: String,
    /// The single operation permitted. Read is the only value this version accepts.
    pub operation: GrantOperation,
    /// RFC3339 ms. A grant is inert before this instant.
    pub valid_from: DateTime<Utc>,
    /// RFC3339 ms. **Required** — a grant with no expiry is not expressible.
    pub valid_to: DateTime<Utc>,
    /// Set when revoked. A revoked grant is retained, never deleted, so the
    /// audit trail can answer what was readable when.
    pub revoked_at: Option<DateTime<Utc>>,
    /// What the grantor relied on when consenting, recorded verbatim.
    pub evidence: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GrantOperation { Read }
```

**Not implemented.** The three functions below are the component's write half; no `src/grants`
module exists and nothing calls them. They are retained as the specification the work will be built
against, not as a description of the current tree. Everything above this line is built and live.

```rust
// src/grants/mod.rs — NOT YET CREATED

/// Issue a grant. The caller must own the fact; owning it is the only authority
/// to grant it, and it is checked against the fact's stored `owner`, never
/// against a value in the request body.
pub async fn issue(ctx: &ScopeContext, req: IssueGrant) -> Result<Grant, AppError>;

/// Revoke a grant the caller issued. Idempotent: revoking an already-revoked
/// grant returns the existing record and is not an error.
pub async fn revoke(ctx: &ScopeContext, grant_id: &str) -> Result<Grant, AppError>;

/// List grants the caller issued, or grants naming the caller as grantee.
/// Paginated per SA-PAGE-01.
pub async fn list(ctx: &ScopeContext, q: GrantQuery) -> Result<Page<Grant>, AppError>;
```

#### Internal Logic

**The read-filter fragment gains a fourth arm — built.** `fact_filter_clause()` in
`src/store/mod.rs` carries it today; `grant_arm_sql()` in the same file exposes the arm's text to
tests. The `SELECT VALUE` projection is `type::record(fact)` rather than the bare `fact` shown below:
`grant.fact` is a string and `fact.id` is a record link, so the bare comparison matches nothing while
every test over the fragment text still passes (FU-026, fixed). The shape is otherwise as written:

```
owner.tenant = $ctenant AND (
    owner.user = $cuser
    OR (visibility = 'team-shared' AND owner.team IN $cteams)
    OR visibility = 'tenant-shared'
    OR id IN (SELECT VALUE fact FROM grant
              WHERE grantee = $cuser
                AND revoked_at IS NONE
                AND valid_from <= $now
                AND valid_to > $now)
)
```

`$now` is bound by the caller from one clock read per request, so every arm of one query agrees on the instant. The sub-select is served by a composite index on `(grantee, revoked_at, valid_to)`.

**A grant never widens a write.** `novis_filter_clause()`, the write path and the forget path are untouched. A grantee may read a granted fact and may do nothing else with it.

**A grant is inert outside its window and after revocation**, and both are evaluated in the query rather than at issue time, so a grant that expires between two requests stops working without anything running.

**Order of evaluation is not security-relevant** — the four arms are a disjunction and any one of them admits. The tenant conjunct is outside the disjunction and admits nothing on its own.

#### Data Model

```sql
DEFINE TABLE grant SCHEMAFULL;
DEFINE FIELD fact        ON grant TYPE string;
DEFINE FIELD grantor     ON grant TYPE object;
DEFINE FIELD grantee     ON grant TYPE string;
DEFINE FIELD purpose     ON grant TYPE string ASSERT string::len($value) BETWEEN 1 AND 512;
DEFINE FIELD operation   ON grant TYPE string ASSERT $value = 'read';
DEFINE FIELD valid_from  ON grant TYPE datetime;
DEFINE FIELD valid_to    ON grant TYPE datetime;
DEFINE FIELD revoked_at  ON grant TYPE option<datetime>;
DEFINE FIELD evidence    ON grant TYPE string;
DEFINE FIELD created_at  ON grant TYPE datetime;

-- Serves the read-filter sub-select. Composite, in the order the filter tests.
DEFINE INDEX grant_grantee_active ON grant FIELDS grantee, revoked_at, valid_to;
-- Serves "list grants over this fact" on the grantor's review screen.
DEFINE INDEX grant_fact ON grant FIELDS fact;
```

The table lives inside the tenant namespace, so a grant cannot name a fact in another tenant — cross-tenant grant is structurally unavailable, which is the same property ADR-011 gives facts. Inter-companion sharing across tenants is therefore **out of scope for this component** and remains the consuming design's OQ-16.

#### Error Table

| Condition | Error | HTTP | Code |
|---|---|---|---|
| Caller does not own the fact named in `fact` | `AppError::Forbidden` | 403 | `SCOPE_FORBIDDEN` |
| Named fact does not exist, or is not readable by the caller | `AppError::NotFound` | 404 | `NOT_FOUND` |
| `valid_to` absent, or not after `valid_from` | `AppError::Validation(ValidationKind::Range, _)` | 400 | `VAL_RANGE` |
| `purpose` empty or longer than 512 characters | `AppError::Validation(ValidationKind::Length, _)` | 400 | `VAL_LENGTH` |
| `operation` is any value other than `read` | `AppError::Validation(ValidationKind::Enum, _)` | 400 | `VAL_ENUM` |
| `grantee` equals the grantor's own user id | `AppError::Validation(ValidationKind::Range, _)` | 400 | `VAL_RANGE` |
| Revoking a grant the caller did not issue | `AppError::Forbidden` | 403 | `SCOPE_FORBIDDEN` |
| Token lacks the `write` operation on issue or revoke | `AppError::InsufficientScope` | 403 | `AUTH_INSUFFICIENT_SCOPE` |
| Vector search requested by a caller relying on a grant arm | `AppError::Validation(ValidationKind::Unsupported, _)` | 400 | `VAL_UNSUPPORTED` |

#### Acceptance Criteria

- **[SA-GRANT-01]** A grant is evaluated inside the store query. No code path fetches a fact and then removes it for want of a grant.
- **[SA-GRANT-02]** Revocation takes effect on the next request with no cache invalidation step, because no cache exists between the filter and the row.
- **[SA-GRANT-03]** A grant names exactly one fact and exactly one grantee. Neither is a set, a pattern or a wildcard.
- **[SA-GRANT-04]** A grant carries a required expiry. A record with no `valid_to` cannot be constructed.
- **[SA-GRANT-05]** A grant confers read only. Write, forget and consolidation paths ignore the `grant` table entirely.
- **[SA-GRANT-06]** A grant does not cascade to facts derived from the granted fact.
- **[SA-GRANT-07]** A revoked grant is retained with `revoked_at` set, never deleted.
- **[SA-GRANT-08]** No stage-1 search signal consults the grant arm. A granted fact is reachable by direct fetch and is not returned by vector, keyword or graph search. **Provisional pending the RISK-020 decision below.**
- **[SA-GRANT-09]** Only the fact's owner may issue a grant over it, checked against the stored owner and never against request-body input.
- **[SA-GRANT-10]** Every issue and every revoke is audited under SA-AUDIT-01, carrying grantor, grantee, fact and purpose.

#### Gherkin Scenarios

```gherkin
Scenario: A grantee reads exactly the granted fact and nothing adjacent
  Given user A owns facts F1 and F2 in tenant T
  And A has granted user B read of F1 until tomorrow
  When B fetches F1
  Then B receives F1
  When B fetches F2
  Then B receives 404 NOT_FOUND

Scenario: Revocation is immediate
  Given A has granted B read of F1
  And B has just read F1 successfully
  When A revokes the grant
  And B fetches F1 again
  Then B receives 404 NOT_FOUND
  And no cache invalidation step ran

Scenario: An expired grant stops working without anything running
  Given A granted B read of F1 with valid_to one second in the future
  When two seconds pass
  And B fetches F1
  Then B receives 404 NOT_FOUND

Scenario: A grant does not reach a derived insight
  Given A has granted B read of F1
  And a consolidated fact C was derived from F1 and F2
  When B fetches C
  Then B receives 404 NOT_FOUND

Scenario: A grantee cannot grant onward
  Given A has granted B read of F1
  When B attempts to grant C read of F1
  Then B receives 403 SCOPE_FORBIDDEN

Scenario: A granted fact is fetchable by id
  Given A has granted B read of F1
  When B fetches F1 by id
  Then B receives F1

Scenario: A granted fact is not returned by search
  Given B holds grants over 3 facts among 10,000 in tenant T
  When B issues a recall query matching all three
  Then none of the three is returned
  And this is the behaviour RISK-020 asks whether we want
```

## Open question — RISK-020

**Should a grant make a fact findable, or only fetchable?**

Raised by the `recall` session on January 2026 while verifying this component against a live store.

| Option | Consequence |
|---|---|
| **(a) Id-scoped only** — today's behaviour, made explicit | Simple and safe. A grantee must already know the fact's id. No search path changes. Rules out any feature where a grantee discovers a granted fact. |
| **(b) Grant arm on the keyword and graph legs, not the vector leg** | Granted facts become findable by the two exact legs. Knowingly breaks RISK-017's invariant that the three signals agree, in the exact direction RISK-017 exists to prevent. |
| **(c) Grant arm on all three legs** | Consistent, and the vector leg under-returns for grantees for the reasons above — the failure that looks like an answer. |

**Neither alternative is free, and the second one is not free in the way it first appears.**

The consuming design's OQ-16 asks whether an inter-companion shared fact travels as a live cross-tenant
read or as a **grant-scoped copy carrying provenance**. It is tempting to read the copy as dissolving
this question: under it the shared fact is owned by the recipient, so `owner.user = $cuser` admits it
and no grant arm touches search at all. That reading is incomplete. **The copy does not remove the
problem; it moves it from search to erasure**, which is where the promise is.

`hard_delete` chases lineage — it collects every fact whose `derived_from` contains the erased id,
removes them, and folds their ids into the `DeletionProof` digest. The collection query is
tenant-bound twice over: it executes inside the caller's namespace, and it carries
`fact_read_filter(ctx)`, whose first conjunct is `owner.tenant = $ctenant`
(`memory/src/store/mod.rs`, `hard_delete`). Under the copy model the copy is owned by the **recipient**,
in the recipient's tenant, while the grantor's erasure runs in the grantor's. The grantor erases their
fact, the recipient retains the copy indefinitely, and the digest silently covers only what was
reachable. This is not a defect to be patched in place: under namespace-per-tenant no in-tenant query
could ever reach the copy.

That matters more than the search gap does, because the erasure claim is a regulatory commitment and
the search gap is a convenience. `docs/design/agentic-memory/06-regulatory.md` states the right to
erasure as verifiable hard deletion including derived summaries and embeddings, with proof — and rests
it on the reasoning that "per-tenant erasure is clean, because dropping the tenant namespace removes
all its data". A cross-tenant copy breaks exactly that reasoning. SA-DELETE-01 additionally requires
that a partial delete must not report success.

**"Per-tenant erasure is clean" is a consequence of ADR-011, not a standing property of the service.**
It holds because one tenant is one namespace, and the mechanism is literal — `drop_tenant_namespace`
issues `REMOVE NAMESPACE IF EXISTS <tenant>` at root level. Two regulatory rows cite ADR-011 by name
(erasure and purpose limitation) and a third depends on it (auditability, whose per-tenant trail is
itself covered by per-tenant erasure). Anything that changes the tenant-to-namespace mapping changes
what those rows are resting on, so the dependency is cited here rather than assumed, and does not have
to be re-found later.

| OQ-16 option | What it costs |
|---|---|
| **Live cross-tenant read** | Erasure stays correct, because there is only ever one fact. Costs the `owner.tenant = $ctenant` invariant — today belt-and-braces behind namespace isolation, but under RFC 03 Stage 2, where many tenants share a namespace, it becomes the only thing separating co-residents. |
| **Grant-scoped copy** | Search works and this question dissolves. Costs cross-tenant erasure, which needs a propagation mechanism per recipient that is unwritten, and which the deletion proof would have to be trusted to reflect. |

**The copy and the Stage 2 topology interact, and each is survivable alone where the pair is not.** The
memory service's RFC 03 Stage 2 maps many tenants onto one namespace, and its own RISK-021 records that
this makes the namespace drop unsafe: under co-residency `REMOVE NAMESPACE` erases every co-resident
tenant, on the code path a data subject invokes to exercise a right. That is the memory service's
finding to resolve and not this component's. It bears on OQ-16 only in combination: if the copy wins
**and** Stage 2 lands, cross-tenant copies are unreachable by erasure in a topology where the namespace
drop that used to be the backstop is itself unsafe. Either alone has a remedy; together they leave
erasure with no correct path.

**If the copy wins**, the cheapest safeguard is for `hard_delete` to return a proof that **names what it
could not reach** rather than a digest that quietly covers only what it could. A proof that admits its
own boundary is defensible; one that does not is worse than no proof.

**Recommendation: (a) id-scoped only, for now.** Not because nobody has asked for the alternatives, but
because both require a decision this codebase is not yet in a position to take — one needs the Stage 2
topology settled, the other needs an erasure-propagation design. Id-scoped access keeps both doors open
and closes neither.
