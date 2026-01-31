# 04 — Domain Model & Data Lifecycle

> **Mode:** draft · **Revision:** 0.12.0 · **Last updated:** January 2026

Field *types* are deferred to `/spec`; this section names entities, their relationships, and their
lifecycle.

## Domain Model

### Fact
- **Purpose.** The unit of memory — a structured assertion the system has learned.
- **Key fields (untyped here).** identity; structured content; owning **User Scope** and
  **visibility** (user-private | team-shared | tenant-shared); **Source** reference; ingestion time;
  validity interval (valid-from, valid-to); confidence; salience; supersession links (supersedes /
  superseded-by); memory class (episodic | semantic | consolidated — **procedural memory is deferred;
  see [00 — Non-goals](./00-overview.md)**); **fact class** — the caller-owned label the write was
  authorised as, opaque to `memory` and distinct from memory class, absent where the write asserted
  none (ADR-019); **collection** — the caller-chosen set within its scope that recall ranks separately
  before fusion, `""` by default, carried also by the Entities, Relationships and Sources a write in
  that collection creates; never an access rule (ADR-020).
- **Relationships.** Owned by exactly one User Scope, with a visibility that may widen read access to
  its Team or Tenant; cites one Source; connects two or more Entities via Relationships; may supersede
  or be superseded by other Facts.
- **Lifecycle.** Created (via Write Pipeline, after the write gate) → indexed (vector + keyword +
  graph) → recalled / reinforced (resets decay) → superseded (validity ended, retained) or decayed
  (salience-floor permitting) → retired or verifiably deleted.

### Entity
- **Purpose.** A node the facts are about — a person, team, system, document subject, or thing.
- **Key fields.** identity; canonical name; aliases; owning Scope. *From revision 0.12.0 (ADR-021):* normalised names (canonical name and aliases), which recall matches against the query.
- **Relationships.** Connected to other Entities by Relationships; referenced by Facts.
- **Lifecycle.** Created or resolved on write (entity resolution: normalise → deduplicate → resolve
  identity); merged when later found to be the same real thing; never silently overwritten.

### Relationship (rich edge)
- **Purpose.** A typed, property-carrying connection between Entities — the graph's load-bearing
  structure.
- **Key fields.** type (e.g. owns, is-lead-of, supersedes); endpoints; validity interval; ingestion
  time; confidence; source; metadata.
- **Relationships.** Joins exactly two Entities (directed); participates in multi-hop traversal.
  *From revision 0.12.0 (ADR-021):* asserted or ended by one or more Facts (the asserting-fact record);
  a person's correction replaces one Relationship with another (**replaces**).
- **Lifecycle.** Created on write; validity ended on supersession (not deleted); decays with its Fact.
  *From revision 0.12.0 (ADR-021):* written only when the write gate admits its asserting Fact; closed
  when no live Fact still asserts it, or when a Fact states it has ended; removed when erasure leaves it
  with no asserting Fact; followed in recall (one hop by default) only through a Fact the reader can read.

### Source (provenance)
- **Purpose.** Where a Fact came from, enabling citation, freshness checks, and trust scoring.
- **Key fields.** identity; origin reference (document/system handle); origin modification marker
  (for freshness); trust signal.
- **Relationships.** Cited by one or more Facts.
- **Lifecycle.** Recorded on write; returned to the agent on request (`include_provenance`) so the
  agent checks source freshness itself (ADR-014); never the authority for access (the source system
  enforces that).

### Scope (Tenant → Team → User)
- **Purpose.** The ownership/access boundary of memory, modelled as a three-level hierarchy:
  **Tenant** (organisation — the hard isolation edge), **Team** (a group within a tenant that may
  share memory), and **User** (individual). The **bridge model** committed in ADR-011.
- **Key fields.** tenant id; team id(s); user id (bound to the OIDC subject claim); the Fact-level
  visibility it grants; partition key.
- **Relationships.** A Tenant contains Teams; a Team contains Users; Facts / Entities / Relationships
  are owned by a User Scope whose visibility may widen read access to a Team or the Tenant. Derived
  from the authenticated identity and membership claims, never from request input.
- **Physical mapping.** Tenant → SurrealDB **namespace** (hard boundary: no cross-tenant query,
  traversal, or vector neighbour). Team / User → **logical** record-level scoping via `PERMISSIONS`
  within the tenant's database, with **database-per-team** as an escape hatch where a team needs
  physical isolation (ADR-011).
- **Lifecycle.** Tenant namespace provisioned on tenant onboarding; user Scope established on first
  authenticated use; reads filtered to (own user facts ∪ facts shared to the user's Team or Tenant);
  a whole Tenant or User is deletable for data-erasure.

### Consolidated Insight (a Fact subtype)
- **Purpose.** A semantic Fact distilled from many episodes.
- **Key fields.** as Fact, plus: derived-from links to source Facts; decaying confidence.
- **Relationships.** References the episodic Facts it summarises; must not outrank them.
- **Lifecycle.** Proposed by the Maintenance Worker → validated against sources → promoted with
  decaying confidence → expires unless re-confirmed (guards against self-reinforcing error).

## Data Lifecycle

- **Origin.** Facts originate from interactions and from source documents read **by the broker as the
  user** — `memory` never reads sources directly, so source access rights are preserved.
- **Storage.** All Facts, Entities, Relationships, Sources live in the Memory Store, partitioned by
  **Tenant** (a namespace each) and scoped by Team / User within. Vectors and keyword indexes are
  derived artefacts, rebuildable from the Facts, and are isolated per tenant (the index is per-table
  within a namespace).
- **Retention.** Bounded over time by graceful decay (Ebbinghaus-style) plus TTL on long-tail
  entries, protected by a salience floor so high-importance Facts survive disuse. Superseded Facts are
  retained for history, not deleted, until explicit erasure.
- **Access.** Hard isolation at the **Tenant** boundary (separate namespace — no cross-tenant read,
  traversal, or vector neighbour). Within a tenant, reads return the user's own facts plus facts
  shared to their Team or the Tenant, enforced by record-level permissions on the authenticated
  membership claims. Externally-sourced content is untrusted until it passes the write gate.
- **Compliance / privacy.** Per-Scope isolation; PII redacted or flagged on the write path where
  feasible; encryption at rest; verifiable deletion (including derived summaries and embeddings) on
  request; every access audited. See [06 — Regulatory](./06-regulatory.md).
