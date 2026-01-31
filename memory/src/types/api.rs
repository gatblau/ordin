//! API request/response payloads (§2C.4).
//!
//! Used by: C8 (HTTP), C6 (recall), C4 (remember).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::domain::{Entity, Fact, MemoryClass, Relationship, Source, Visibility};
use crate::types::ports::LinkRole;

#[derive(Deserialize)]
pub struct RecallRequest {
    /// 1..=4096 chars when `searches` is absent; "" otherwise — exactly one of a non-empty `query` and
    /// a non-empty `searches` (SA-SUBQ-01, ADR-020).
    #[serde(default)]
    pub query: String,
    /// 1..=4 typed sub-queries, the first weighted 2.0 (SA-SUBQ-01, SA-SUBQ-02; ADR-020).
    // spec: components/retrieval-engine#SA-SUBQ-01
    #[serde(default)]
    pub searches: Option<Vec<SubQuery>>,
    #[serde(default)]
    pub filters: RecallFilters,
    /// [1,50], default 10 (SA-CAP-01).
    #[serde(default = "default_result_cap")]
    pub result_cap: u8,
    /// Opaque, from a prior meta.next_cursor.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Opt-in: attach each sourced fact's `origin_ref` + `modification_marker` so the agent can
    /// check source freshness itself (SA-PROV-01, ADR-014). Default false keeps the response lean.
    #[serde(default)]
    pub include_provenance: bool,
}

#[derive(Deserialize, Default)]
pub struct RecallFilters {
    pub memory_class: Option<MemoryClass>,
    pub visibility: Option<Visibility>,
    /// Restrict to facts touching this entity id.
    pub entity: Option<String>,
    /// As-of query into the bi-temporal history.
    pub valid_at: Option<DateTime<Utc>>,
    /// 1–32 distinct labels, each matching SA-COLL-01; "" names the default collection.
    /// Absent = every collection the caller can read, at most 32 in play, else the recall is refused.
    /// Applied before every signal (ADR-020).
    // spec: components/memory-store#SA-COLL-02
    #[serde(default)]
    pub collections: Option<Vec<String>>,
}

/// One typed sub-query of a recall (ADR-020): a keyword (`lex`) or meaning (`vec`) search written by
/// the agent.
#[derive(Deserialize, Clone)]
pub struct SubQuery {
    #[serde(rename = "type")]
    pub kind: SubQueryKind,
    /// 1..=4096 chars.
    pub query: String,
}

/// The kind of a [`SubQuery`].
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SubQueryKind {
    Lex,
    Vec,
}

/// Wrapped in `Success<RecallResponse>`.
#[derive(Serialize)]
pub struct RecallResponse {
    pub facts: Vec<RankedFact>,
    /// The links that led to facts on the page, at most 20 (C6 step 10″, ADR-021); absent when recall
    /// followed none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<RecallLink>,
}

/// One link recall followed to reach facts on the page (C6 step 10″, ADR-021).
#[derive(Serialize, Clone)]
pub struct RecallLink {
    pub relationship: Relationship,
    pub from_name: String,
    pub to_name: String,
    /// The page's fact ids this link led to.
    pub reached: Vec<String>,
}

#[derive(Serialize)]
pub struct RankedFact {
    pub fact: Fact,
    /// Final ranking score [0,1].
    pub score: f64,
    /// Present only when the request set `include_provenance` and the fact cites a source
    /// (SA-PROV-01, ADR-014). Lets the agent run its own source-freshness check; recall asserts no
    /// freshness verdict of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceProvenance>,
    /// Whether the fact is fading (SA-GRAPH-07, `maintenance::is_fading`).
    pub fading: bool,
    /// The live facts recorded in conflict with this one that the caller can read, at most 8, smaller
    /// id first (retrieval-engine amendment ADR-022).
    // spec: phase-1-analysis#SA-TRIPLE-13
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
}

/// Returned per sourced fact when `include_provenance` is set (SA-PROV-01, ADR-014).
#[derive(Serialize)]
pub struct SourceProvenance {
    /// Document/system handle the agent resolves.
    pub origin_ref: String,
    /// ETag / Last-Modified token captured at write time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modification_marker: Option<String>,
}

#[derive(Deserialize)]
pub struct RememberRequest {
    /// Structured assertion object the agent has already extracted (no LLM extraction; ADR-015).
    pub content: serde_json::Value,
    pub source: Option<SourceInput>,
    /// Optional caller-asserted class; defaults to `Episodic` (SA-WRITE-STRUCTURED-01).
    #[serde(default)]
    pub memory_class: Option<MemoryClass>,
    /// Optional caller-owned label naming the class of assertion being made. Opaque to `memory`:
    /// it is compared against the token's authority claim and never interpreted. Absent means an
    /// unclassed write, admitted on the `memory.write` scope alone. 1..=128 chars when present.
    // spec: phase-1-analysis#SA-AUTHZ-01
    #[serde(default)]
    pub fact_class: Option<String>,
    /// The visibility the written fact carries; absent writes `user-private`, as before. Honoured only
    /// for the in-process ordin caller (`ctx.client == Some("ordin-in-process")`); any other caller
    /// that sets it is refused with `VAL_INVALID_BODY` (ordin change M-6 a).
    #[serde(default)]
    pub visibility: Option<Visibility>,
    /// SA-COLL-01; absent = "" (the default collection). Carried onto the fact and any entity or
    /// source the write creates (ADR-020).
    // spec: components/memory-store#SA-COLL-01
    #[serde(default)]
    pub collection: Option<String>,
    /// The things the fact is about, 0..=16 (ADR-021).
    #[serde(default)]
    pub entities: Vec<EntityAssertion>,
    /// Links the agent states between those things, 0..=16 (ADR-021).
    #[serde(default)]
    pub links: Vec<LinkAssertion>,
    /// The facts a consolidated fact summarises: none, or 2..=64 distinct `fact:<uuid>` ids, only with
    /// `memory_class = consolidated` (ADR-021).
    #[serde(default)]
    pub derived_from: Vec<String>,
    /// The trust the write gate reads for this write in place of its source's (SA-WGATE-02). Honoured
    /// only for the in-process ordin caller; any other caller that sets it is refused with
    /// `VAL_INVALID_BODY`, as for `visibility`.
    #[serde(default)]
    pub gate_trust: Option<f64>,
    /// Who among the owner's callers makes this write (SA-GRAPH-14). In-process only, as `gate_trust`.
    #[serde(default)]
    pub author: Option<String>,
    /// A subject has one value of this predicate at a time (SA-TRIPLE-05).
    // spec: phase-1-analysis#SA-TRIPLE-05
    #[serde(default)]
    pub single_valued: bool,
    /// The earlier value stopped being true; requires `single_valued` (SA-TRIPLE-05).
    #[serde(default)]
    pub changed: bool,
    /// This triple stopped being true (SA-TRIPLE-05, SA-TRIPLE-07).
    #[serde(default)]
    pub ended: bool,
    /// The writer's confidence in the fact, [0,1]; the fact carries it and its implied link takes it;
    /// absent = 1.0 (SA-TRIPLE-14).
    // spec: phase-1-analysis#SA-TRIPLE-14
    #[serde(default)]
    pub confidence: Option<f64>,
    /// When the fact became true; not later than the write; absent = the time of the write
    /// (SA-TRIPLE-14).
    #[serde(default)]
    pub valid_from: Option<DateTime<Utc>>,
}

/// The content of a non-passage write: one subject, one predicate, one object (SA-TRIPLE-01).
// spec: phase-1-analysis#SA-TRIPLE-01
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Triple {
    /// 1..=512 characters after NFC and trim.
    pub subject: String,
    /// 1..=128 characters after `normalise_predicate`.
    pub predicate: String,
    pub object: TripleObject,
}

/// A triple's object: a thing or a typed value (SA-TRIPLE-02).
// spec: phase-1-analysis#SA-TRIPLE-02
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TripleObject {
    /// 1..=512.
    Entity {
        name: String,
    },
    /// 1..=1000, no control character other than `\n`.
    Text {
        value: String,
    },
    /// A decimal string; unit 1..=32.
    Number {
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    /// A decimal string; `^[A-Z]{3}$`.
    Money {
        amount: String,
        currency: String,
    },
    /// `YYYY` | `YYYY-MM` | `YYYY-MM-DD`.
    Date {
        value: String,
    },
    Boolean {
        value: bool,
    },
}

/// `fact_conflicts` input: 1..=100 distinct `fact:<uuid>` ids (service-layer amendment ADR-022).
// spec: phase-1-analysis#SA-TRIPLE-13
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct FactConflictsRequest {
    pub ids: Vec<String>,
}

/// `fact_conflicts` output: one item per requested id, in request order.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct FactConflictsResult {
    pub items: Vec<FactConflictItem>,
}

/// The readable live facts recorded in conflict with `fact`, at most 8, smaller id first.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct FactConflictItem {
    pub fact: String,
    pub with: Vec<String>,
}

/// One thing a remembered fact is about (ADR-021): a name and other names for it.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntityAssertion {
    /// 1..=512 characters after NFC and trim.
    pub name: String,
    /// 0..=32, each 1..=512 characters.
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// One link a remembered fact states between two things it names (ADR-021).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LinkAssertion {
    /// A name or alias of `entities`, or `content.subject` / `content.object`.
    pub from: String,
    /// 1..=128 characters after normalisation.
    pub kind: String,
    pub to: String,
    /// The link is no longer true: the live link it names is closed.
    #[serde(default)]
    pub ended: bool,
    /// [0,1]; default 1.0.
    #[serde(default)]
    pub confidence: Option<f64>,
}

#[derive(Deserialize)]
pub struct SourceInput {
    pub origin_ref: String,
    pub modification_marker: Option<String>,
}

/// Async (ADR-004).
// `Deserialize` is derived alongside `Serialize` so the Service Layer (C9) can round-trip the stored
// idempotency body back into the typed payload on a replay; the wire shape is unchanged.
#[derive(Serialize, Deserialize)]
pub struct WriteAck {
    pub job_id: String,
    pub status: JobAckStatus,
}

/// AlreadyAccepted => idempotent replay.
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum JobAckStatus {
    Accepted,
    AlreadyAccepted,
}

/// Acknowledgement returned by `POST /v1/memories/{id}/retire` (C8). A replay within the idempotency
/// window returns the original `retired_at`.
#[derive(Serialize, Deserialize)]
pub struct RetireAck {
    pub record_id: String,
    pub retired_at: DateTime<Utc>,
}

/// Service capabilities returned by `GET /v1` (C8). `openapi` is the absolute URL of `/openapi.json`.
#[derive(Serialize)]
pub struct Capabilities {
    pub service: String,
    pub version: String,
    pub operations: Vec<String>,
    pub openapi: String,
}

#[derive(Serialize, Deserialize)]
pub struct DeletionProof {
    pub deleted_at: DateTime<Utc>,
    pub record_id: String,
    /// Ids of removed derived summaries.
    pub derived_removed: Vec<String>,
    pub embeddings_removed: u32,
    /// Sha256 hex over sorted removed ids (SA-DELETE-01).
    pub digest: String,
    /// Ids of the relationships `hard_delete` or `delete_relationship` removed (ADR-021).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships_removed: Vec<String>,
    /// Ids of the entities `hard_delete` removed because nothing else used them.
    // spec: phase-1-analysis#SA-DELETE-02
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities_removed: Vec<String>,
    /// Ids of the sources `hard_delete` removed because nothing else used them.
    // spec: phase-1-analysis#SA-DELETE-02
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources_removed: Vec<String>,
}

/// Evidence that one tenant's data has been erased (ADR-018).
///
/// Distinct from `DeletionProof`, which covers one fact and digests the record ids it removed. A
/// whole-tenant erasure claims that *nothing of this tenant remains*, and the verification pass
/// discharges that claim directly by re-counting every table on the tenant predicate and requiring
/// zero. The digest therefore records what was removed — per-table counts — rather than committing to
/// every removed id, which would mean holding a 25,000-fact tenant's ids in memory to prove something
/// the re-count already proves.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TenantErasureProof {
    /// Server-set, RFC3339 ms.
    pub erased_at: DateTime<Utc>,
    /// The logical tenant erased.
    pub tenant: String,
    /// The physical namespace the erasure predicate ran in.
    pub namespace: String,
    /// Rows removed per table. A `BTreeMap` so the digest input has one stable order.
    pub removed: BTreeMap<String, u64>,
    /// Sum over `removed`.
    pub total_removed: u64,
    /// Sha256 hex over `tenant=`, `namespace=`, then one `<table>=<count>` line per entry of
    /// `removed` in key order (SA-ERASE-01).
    pub digest: String,
}

/// `Service::list_facts` request (ordin change M-6 b). `cursor` is the opaque `next_cursor` of the
/// previous page; `include_retired` also lists facts whose validity has ended.
#[derive(Deserialize)]
pub struct ListFactsRequest {
    pub cursor: Option<String>,
    #[serde(default)]
    pub include_retired: bool,
}

/// One page of `Service::list_facts`: the caller's read-filtered facts, newest `ingested_at` first,
/// at most 100; `next_cursor` is `None` on the last page.
#[derive(Serialize)]
pub struct ListFactsPage {
    pub facts: Vec<Fact>,
    pub next_cursor: Option<String>,
}

/// `Service::export_scope` request (ordin change M-7 a).
#[derive(Deserialize)]
pub struct ExportRequest {
    pub cursor: Option<String>,
}

/// One page of `Service::export_scope`: at most 200 read-filtered facts ordered by id, closed ones
/// included, with every entity, source and relationship they name.
#[derive(Serialize)]
pub struct ExportPage {
    pub facts: Vec<Fact>,
    pub entities: Vec<Entity>,
    pub sources: Vec<Source>,
    pub relationships: Vec<Relationship>,
    pub next_cursor: Option<String>,
}

/// `Service::import_facts` request (ordin change M-7 b): structured facts with their own ids,
/// imported without extraction.
#[derive(Deserialize)]
pub struct ImportRequest {
    pub facts: Vec<Fact>,
    pub entities: Vec<Entity>,
    pub sources: Vec<Source>,
    pub relationships: Vec<Relationship>,
}

/// The outcome of one imported item.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub enum ImportResult {
    Admitted,
    Unchanged,
    Quarantined,
    Rejected,
    Invalid { reason: String },
}

/// `Service::import_facts` result: one `(id, outcome)` per fact, in request order, then one
/// `Invalid` per entity, source or relationship of the request that was refused and not written.
#[derive(Serialize, Debug)]
pub struct ImportReport {
    pub results: Vec<(String, ImportResult)>,
}

// --- Entities, links and graph recall (ADR-021): the request and result types of the new Service
// operations (2C.4). ---

/// `Service::fact_history` request.
#[derive(Deserialize)]
pub struct FactHistoryRequest {
    pub id: String,
}

/// `Service::fact_origins` request.
#[derive(Deserialize)]
pub struct FactOriginsRequest {
    pub id: String,
}

/// `Service::list_entities` request: `search` 1..=200 characters; `cursor` the previous page's.
#[derive(Deserialize, Default)]
pub struct ListEntitiesRequest {
    pub search: Option<String>,
    pub cursor: Option<String>,
}

/// `Service::entity_detail` request.
#[derive(Deserialize)]
pub struct EntityDetailRequest {
    pub id: String,
}

/// `Service::correct_link` request: the relationship to replace and what replaces it.
#[derive(Deserialize, Clone)]
pub struct CorrectLinkRequest {
    pub id: String,
    /// 1..=128 characters after normalisation.
    pub kind: String,
    pub from: String,
    pub to: String,
}

/// A fact's versions, oldest first, at most 50.
#[derive(Serialize)]
pub struct FactHistory {
    pub versions: Vec<FactVersion>,
}

/// One version of a fact with the links it asserted or ended.
#[derive(Serialize)]
pub struct FactVersion {
    pub fact: Fact,
    pub links: Vec<(Relationship, LinkRole)>,
}

/// One fact a summary was made from: the fact when readable, and whether it is no longer valid.
#[derive(Serialize)]
pub struct OriginFact {
    pub id: String,
    pub fact: Option<Fact>,
    pub ended: bool,
}

fn default_result_cap() -> u8 {
    10
}
