//! C4 — Write Pipeline. The asynchronous consumer of `ExtractFact` work jobs from the durable work
//! queue (C2). It turns raw, untrusted `remember` content into a clean, scoped, provenance-tagged
//! `Fact` in the Memory Store (C1), or into a quarantined / rejected record when the content cannot be
//! trusted.
//!
//! The component is a **linear step pipeline driven by a claim/lease loop** (C4 *Approach*): one async
//! task per worker calls `WorkQueue::claim(&[JobKind::ExtractFact], lease)`; each claimed job flows
//! through eight ordered step functions in fixed order:
//!
//!   1. Filter noise         — drop empty / low-signal content (data minimisation).
//!   2. Intake               — wrap the structured, agent-asserted content as a Fact (no LLM; ADR-015).
//!   3. Normalise            — canonicalise content (key order, whitespace, NFC, RFC3339 ms).
//!   4. Entity-resolve       — rules -> ML -> create-new ladder (v1; never silent overwrite).
//!   5. Score                — salience + confidence in [0,1] (SA-SCORE-01).
//!   6. PII scan             — redact high-confidence spans in place, else flag `pii_review` (SA-PII-01).
//!   7. Write gate           — trust bands admit/quarantine/reject (SA-WGATE-01, ADR-008).
//!   8. Embed + persist      — `EmbeddingClient::embed` then `MemoryStore::put_fact` (idempotent id).
//!
//! It never runs on the read path (ADR-004): a slow or failed write is retried with backoff behind the
//! queue and is dead-lettered after the attempt cap (SA-QUEUE-02), and a write never blocks a read.
//!
//! **Security.** Untrusted-source content is treated as **data, never instructions** (ADR-008): the
//! imperative-pattern detector caps instruction-like content strictly below the quarantine floor so it
//! can never be admitted, and extracted content is never evaluated, executed, or used to steer pipeline
//! control flow. Scope is taken only from the job's `ScopeRef` (derived by C3 at enqueue), never from
//! request-body content. Every store write goes through parameterised queries. No content, PII span
//! text, secret, or token is ever logged — only counts and ids.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{AppError, ValidationKind};
use crate::text::normalise_name;
use crate::types::api::{EntityAssertion, LinkAssertion, SourceInput, Triple, TripleObject};
use crate::types::domain::{
    AliasFolder, Entity, Fact, MemoryClass, Relationship, Source, TripleKey, Visibility,
};
use crate::types::job::{JobKind, WorkJob};
use crate::types::ports::{
    EmbeddingClient, EntityMention, ExtractedFact, LinkRole, PiiDetector, PiiSpan, ProviderError,
    StoragePort, StoreError,
};
use crate::types::scope::{OpSet, ScopeContext, ScopeRef};

/// The outcome of one job (one persisted/quarantined/rejected fact), used for logging, metrics, and
/// tests (C4 *Public Interface*).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteOutcome {
    Persisted,
    Quarantined,
    Rejected,
    FilteredNoise,
    /// An erasure removed the job while it ran: nothing it wrote stays, and there is no job to complete.
    Erased,
}

/// Decision returned by the write gate, separating the three audited outcomes (C4 *Public Interface*).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateDecision {
    Admit,
    Quarantine,
    Reject,
}

/// Result of the imperative-pattern detector (C4 *Public Interface*).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InstructionLikelihood {
    /// `true` => the trust score is capped strictly below `trust_quarantine`.
    pub is_instruction_like: bool,
    /// Count of imperative patterns matched (for audit).
    pub matched_patterns: u32,
}

/// A gate-quarantined record persisted to the `quarantine` table (C4 *Data Model*).
#[derive(Clone, Serialize, Deserialize)]
pub struct QuarantineRecord {
    /// "quarantine:<uuidv7>".
    pub id: String,
    /// Redacted content (object) — PII already removed.
    pub content: Json,
    /// Entity ids: none on a write held since F-005, which creates no entity; `entities_asserted` has
    /// what it stated.
    pub entities: Vec<String>,
    pub source_id: Option<String>,
    pub memory_class: MemoryClass,
    /// The caller-owned class the write claimed, so a reviewer of a held write sees what it claimed
    /// to be and not only what it contained. Added by migration `0006`; `quarantine` is SCHEMAFULL,
    /// so without that DDL a classed quarantine write would be rejected outright and the held record
    /// lost entirely — not merely the label.
    // spec: phase-1-analysis#SA-AUTHZ-09
    #[serde(default)]
    pub fact_class: Option<String>,
    pub owner: ScopeRef,
    /// [0,1] — the gate's trust score.
    pub trust_score: f64,
    /// [0,1].
    pub confidence: f64,
    /// [0,1].
    pub salience: f64,
    pub is_instruction_like: bool,
    pub pii_review: bool,
    pub idempotency_key: Option<String>,
    /// The fact this write would have been, which is what the host knows it by. `None` before
    /// migration 0010.
    // spec: phase-1-analysis#SA-QUAR-01
    #[serde(default)]
    pub fact_id: Option<String>,
    /// Audit: why quarantined.
    pub reason: String,
    pub quarantined_at: DateTime<Utc>,
    /// The collection of the held write (ADR-020, SA-COLL-01); declared on the SCHEMAFULL
    /// `quarantine` by migration `0007`.
    // spec: components/memory-store#SA-COLL-01
    #[serde(default)]
    pub collection: String,
    /// The entities the held write asserted (ADR-021; column `entities_asserted`, migration 0008).
    // `entities` is already this record's resolved-ids field, so the assertions take the column names.
    #[serde(default)]
    pub entities_asserted: Vec<EntityAssertion>,
    /// The links the held write asserted (column `links_asserted`), so a release writes them.
    #[serde(default)]
    pub links_asserted: Vec<LinkAssertion>,
    /// The facts a held consolidated write summarises.
    #[serde(default)]
    pub derived_from: Vec<String>,
}

/// Step 4's resolution with the names it resolved (ADR-021): the fact's entity ids, and every
/// normalised name, alias, subject and object the write stated → the entity id it resolved to.
pub struct ResolvedEntities {
    pub ids: Vec<String>,
    pub by_name: std::collections::HashMap<String, String>,
    /// The entities this resolution created, which an overtaken write removes again.
    pub created: Vec<String>,
}

/// The entities, links and summary origins a write states (ADR-021), carried through steps 2–8′.
#[derive(Clone, Default)]
struct Assertions {
    entities: Vec<EntityAssertion>,
    links: Vec<LinkAssertion>,
    derived_from: Vec<String>,
    /// The caller's teams at intake, for reading `derived_from`; empty for any other write.
    origin_teams: Vec<String>,
    /// SA-GRAPH-14.
    author: Option<String>,
    /// The write's triple (ADR-022); `None` for a passage.
    triple: Option<Triple>,
    /// SA-TRIPLE-05.
    single_valued: bool,
    changed: bool,
    ended: bool,
    /// SA-TRIPLE-14: the writer's confidence and start.
    confidence: Option<f64>,
    valid_from: Option<DateTime<Utc>>,
}

impl Assertions {
    /// The scope a summary's origins are read under: the job's, with the teams intake read them under.
    fn origin_ctx(&self, ctx: &ScopeContext) -> ScopeContext {
        let mut origin = ctx.clone();
        if !self.origin_teams.is_empty() {
            origin.teams = self.origin_teams.clone();
        }
        origin
    }
}

/// `by_name` with every asserted alias → its entity, so a link may name an entity by its alias.
fn alias_names(
    asserted: &[EntityAssertion],
    mut by_name: std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    for a in asserted {
        let Some(id) = by_name.get(&normalise_name(&a.name)).cloned() else {
            continue;
        };
        for alias in &a.aliases {
            let n = normalise_name(alias);
            if !n.is_empty() {
                by_name.entry(n).or_insert_with(|| id.clone());
            }
        }
    }
    by_name
}

/// Validated write-pipeline configuration (C4 *Public Interface*). The spec declares this `Copy`; it
/// carries `embed_model_version: String` here (used at persist time), so it is `Clone` rather than
/// `Copy`.
#[derive(Clone)]
pub struct WritePipelineConfig {
    /// `MEMORY_PASSAGE_CLASS` — copied from `Config.passage_class` (SA-TRIPLE-11).
    // spec: phase-1-analysis#SA-TRIPLE-11
    pub passage_class: String,
    /// `MEMORY_TRUST_ADMIT` — a trust score `>=` this admits.
    pub trust_admit: f64,
    /// `MEMORY_TRUST_QUARANTINE` — `quarantine <= score < admit` quarantines; `< quarantine` rejects.
    pub trust_quarantine: f64,
    /// `MEMORY_PII_REDACT_CONF` — span confidence `>=` this is redacted; below sets `pii_review=true`.
    pub pii_redact_conf: f64,
    /// `MEMORY_EMBED_DIM` — the embed vector length must equal this (SA-EMBED-01).
    pub embed_dim: u32,
    /// `MEMORY_JOB_MAX_ATTEMPTS` — retry cap before dead-letter (owned by C2; read for logging).
    pub max_attempts: u32,
    /// `MEMORY_SOURCE_TRUST_DEFAULT` — prior trust for a newly-seen source.
    pub source_trust_default: f64,
    /// `MEMORY_EMBED_MODEL_VERSION` — stored alongside the embedding for re-embed bookkeeping (C7).
    pub embed_model_version: String,
    /// Claim lease length (default 30 s).
    pub claim_lease: Duration,
    /// Per-job wall-clock budget (default 30 s).
    pub per_job_budget: Duration,
}

impl WritePipelineConfig {
    /// Build the pipeline config from the loaded §2D configuration.
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            passage_class: cfg.passage_class.clone(),
            trust_admit: cfg.trust_admit,
            trust_quarantine: cfg.trust_quarantine,
            pii_redact_conf: cfg.pii_redact_conf,
            embed_dim: cfg.embed_dim,
            max_attempts: cfg.job_max_attempts,
            source_trust_default: cfg.source_trust_default,
            embed_model_version: cfg.embed_model_version.clone(),
            claim_lease: Duration::from_secs(30),
            per_job_budget: Duration::from_secs(30),
        }
    }
}

/// Step 7a as a free function over any JSON (ordin change M-10 i): imperative/instruction-like content
/// over the content's string values (case-insensitive), and known phrases in its object keys
/// (SA-PII-02). `is_instruction_like` is `true` when `matched_patterns >= 1` (ADR-008).
/// [`WritePipeline::detect_instruction`] calls it; its result is unchanged.
// spec: components/write-pipeline#SA-PII-02
pub fn instruction_likelihood(content: &Json) -> InstructionLikelihood {
    let mut matched = 0u32;
    for_each_string(content, &mut |s| {
        matched += count_instruction_patterns(s);
    });
    // spec: phase-1-analysis#SA-PII-02
    // A key is read for phrases only: `run`, `set` and `output` are ordinary field
    // names, so the leading-verb rule would refuse them. No pattern can match a number, so a
    // number is not read here; a phrase split across keys and values is not looked for.
    for_each_key(content, &mut |k| matched += count_key_phrases(k));
    InstructionLikelihood {
        is_instruction_like: matched >= 1,
        matched_patterns: matched,
    }
}

/// What every redaction token of step 6 starts with.
const REDACTED_MARK: &str = "‹redacted:";

/// The longest entity name the store accepts, in characters.
const NAME_MAX_CHARS: usize = 512;

/// The ML-tier string-similarity admit threshold (C4 step 4): a single candidate scoring at or above
/// this resolves to the existing entity; otherwise the mention escalates to create-new.
const ML_SIMILARITY_ADMIT: f64 = 0.92;

/// The validated `remember` job payload deserialised from `WorkJob::payload` (step 0). Content is a
/// structured, agent-asserted assertion object (no LLM extraction; ADR-015).
#[derive(Deserialize)]
struct JobPayload {
    content: Json,
    #[serde(default)]
    source: Option<SourceInput>,
    /// Optional caller-asserted class; defaults to `Episodic` (SA-WRITE-STRUCTURED-01).
    #[serde(default)]
    memory_class: Option<MemoryClass>,
    /// The caller-owned class C9 **already authorised** against the token's `memory_fact_authority`
    /// claim. C4 carries it to storage and judges nothing about it: no vocabulary, no validation, no
    /// value treated specially. A job carrying none is an unclassed write and stays unclassed.
    // spec: phase-1-analysis#SA-AUTHZ-09
    #[serde(default)]
    fact_class: Option<String>,
    /// The visibility C9 already admitted (ordin change M-6 a: only the in-process ordin caller may
    /// set it). A job carrying none writes `user-private`, as every write did before.
    #[serde(default)]
    visibility: Option<Visibility>,
    /// The collection C9 already validated (ADR-020, SA-COLL-01); absent = "" (the default
    /// collection). Carried onto the fact, any entity or source the write creates, and a held record.
    #[serde(default)]
    collection: Option<String>,
    /// The things the fact is about (ADR-021); validated by C9.
    #[serde(default)]
    entities: Vec<EntityAssertion>,
    /// The links the agent states between them (ADR-021); validated by C9.
    #[serde(default)]
    links: Vec<LinkAssertion>,
    /// SA-WGATE-02: what the gate reads in place of the source's trust; set by C9 for the in-process
    /// caller alone.
    #[serde(default)]
    gate_trust: Option<f64>,
    /// SA-GRAPH-14; set by C9 for the in-process caller alone.
    #[serde(default)]
    author: Option<String>,
    /// The facts a consolidated write summarises (ADR-021); checked by C9 at intake.
    #[serde(default)]
    derived_from: Vec<String>,
    /// The teams intake read `derived_from` under. Set by the Service from the verified caller,
    /// never from the request.
    #[serde(default)]
    origin_teams: Vec<String>,
    /// The writer's statements (SA-TRIPLE-05), validated by C9.
    // spec: components/write-pipeline#SA-TRIPLE-05
    #[serde(default)]
    single_valued: bool,
    #[serde(default)]
    changed: bool,
    #[serde(default)]
    ended: bool,
    /// SA-TRIPLE-14, validated by C9.
    // spec: components/write-pipeline#SA-TRIPLE-14
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default)]
    valid_from: Option<DateTime<Utc>>,
}

/// Step 0's defence for the ADR-021 fields: the bounds C9 validated (a breach is a malformed payload).
fn assertions_within_bounds(p: &JobPayload) -> bool {
    let name_ok = |n: &str| {
        let c = n.trim().chars().count();
        (1..=512).contains(&c)
    };
    p.entities.len() <= 16
        && p.entities.iter().all(|e| {
            name_ok(&e.name) && e.aliases.len() <= 32 && e.aliases.iter().all(|a| name_ok(a))
        })
        && p.links.len() <= 16
        && p.links.iter().all(|l| {
            let k = crate::text::normalise_kind(&l.kind).chars().count();
            (1..=128).contains(&k) && l.confidence.is_none_or(|c| (0.0..=1.0).contains(&c))
        })
        && (p.derived_from.is_empty() || (2..=64).contains(&p.derived_from.len()))
}

/// The single existing kind a link kind merges into (SA-GRAPH-03): an exact match, else the one kind
/// with bigram Dice ≥ 0.92, else the normalised kind itself.
// spec: components/memory-store#SA-GRAPH-03
fn merge_kind(kind: &str, existing: &[String]) -> String {
    let k = crate::text::normalise_kind(kind);
    if existing.contains(&k) {
        return k;
    }
    let close: Vec<&String> = existing
        .iter()
        .filter(|e| crate::text::dice_bigram(&k, e) >= ML_SIMILARITY_ADMIT)
        .collect();
    if close.len() == 1 {
        return close[0].clone();
    }
    k
}

/// The most facts one write ends, or records a conflict with (SA-TRIPLE-06, SA-TRIPLE-08).
const CHANGE_MAX: usize = 16;

/// Whether `l` is the link `t`'s entity object asserts: its ends equal after `normalise_name`, its kind
/// equal to the predicate (SA-TRIPLE-04).
// spec: phase-1-analysis#SA-TRIPLE-04
fn is_implied_link(l: &LinkAssertion, t: &Triple) -> bool {
    let Some(object) = crate::triple::object_name(t) else {
        return false;
    };
    crate::text::normalise_predicate(&l.kind) == t.predicate
        && normalise_name(&l.from) == normalise_name(&t.subject)
        && normalise_name(&l.to) == normalise_name(object)
}

/// Step 6″: the scan dropped the subject or the entity object (steps 6′ and 6), or changed a number,
/// money or date value — the triple then states nothing checkable.
// spec: components/write-pipeline#SA-TRIPLE-02
fn triple_redacted(t: &Triple, ef: &ExtractedFact, redacted: &Json) -> bool {
    let mentioned = |name: &str| {
        let n = normalise_name(name);
        ef.entities.iter().any(|m| normalise_name(&m.surface) == n)
    };
    let text_at = |pointer: &str| redacted.pointer(pointer).and_then(Json::as_str);
    let marked = |pointer: &str| text_at(pointer).is_some_and(|v| v.contains(REDACTED_MARK));
    if !mentioned(&t.subject) || marked("/subject") {
        return true;
    }
    match &t.object {
        TripleObject::Entity { name } => !mentioned(name) || marked("/object/name"),
        TripleObject::Number { value, .. } => text_at("/object/value") != Some(value.as_str()),
        TripleObject::Money { amount, .. } => text_at("/object/amount") != Some(amount.as_str()),
        TripleObject::Date { value } => text_at("/object/value") != Some(value.as_str()),
        TripleObject::Text { .. } | TripleObject::Boolean { .. } => false,
    }
}

/// Step 4″: the triple's indexed parts from the names the write resolved (SA-TRIPLE-02); `None` when
/// the subject or an entity object resolved to no entity.
// spec: phase-1-analysis#SA-TRIPLE-02
pub(crate) fn triple_key_of(
    t: &Triple,
    by_name: &std::collections::HashMap<String, String>,
) -> Option<TripleKey> {
    let subject_entity = by_name.get(&normalise_name(&t.subject))?.clone();
    let (object_kind, value, object_unit) = crate::triple::canonical_object(&t.object);
    let (object_value, object_entity) = match &t.object {
        TripleObject::Entity { name } => {
            let id = by_name.get(&normalise_name(name))?.clone();
            (id.clone(), Some(id))
        }
        _ => (value.unwrap_or_default(), None),
    };
    Some(TripleKey {
        subject_entity,
        predicate: t.predicate.clone(),
        object_kind,
        object_value,
        object_unit,
        object_entity,
        author: None,
    })
}

/// The runtime entrypoint and step functions of the Write Pipeline (C4 *Public Interface*).
pub struct WritePipeline {
    /// The storage port: facts, entities, sources, the work queue and the C4-owned `quarantine` sink.
    storage: Arc<dyn StoragePort>,
    embed: Arc<dyn EmbeddingClient>,
    pii: Arc<dyn PiiDetector>,
    cfg: WritePipelineConfig,
}

impl WritePipeline {
    /// Construct with injected dependencies and validated config. The storage port carries the
    /// C4-owned `quarantine` sink. No LLM: content arrives structured (ADR-015).
    pub fn new(
        storage: Arc<dyn StoragePort>,
        embed: Arc<dyn EmbeddingClient>,
        pii: Arc<dyn PiiDetector>,
        cfg: WritePipelineConfig,
    ) -> Self {
        Self {
            storage,
            embed,
            pii,
            cfg,
        }
    }

    /// The consumer loop. Claims `ExtractFact` jobs and processes each to a terminal queue state. Runs
    /// until `cancel` is triggered. Never returns on a transient queue error — it logs and re-claims
    /// after a short delay.
    pub async fn run(&self, cancel: CancellationToken) {
        // Round-robin position across tenants, carried between ticks so no tenant starves behind a
        // busy one (RISK-016 — `claim` now names its tenant rather than inheriting a session).
        let mut cursor = 0usize;
        loop {
            if cancel.is_cancelled() {
                return;
            }
            let tenants = match self.storage.list_tenants().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(target: "memory", error = %e, "write.consumer.list_tenants_failed");
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                        _ = cancel.cancelled() => return,
                    }
                    continue;
                }
            };
            // A per-tenant claim failure is logged and skipped inside the helper rather than surfaced,
            // so one unreachable namespace cannot stall every other tenant's work. That is why there
            // is no error arm here any more: the only outcomes are a job or an idle round.
            let claimed = crate::queue::claim_across_tenants(
                self.storage.as_ref(),
                &tenants,
                &[JobKind::ExtractFact],
                self.cfg.claim_lease,
                &mut cursor,
            )
            .await;
            match claimed {
                Some(job) => {
                    let ctx = self.ctx_from_job(&job);
                    match self.process(&ctx, &job).await {
                        Ok(outcome) => {
                            tracing::debug!(
                                target: "memory",
                                job_id = %job.id,
                                outcome = ?outcome,
                                "write.process.done"
                            );
                        }
                        Err(e) => self.fail_job(&job, &e).await,
                    }
                }
                None => {
                    // No claimable job in any tenant; back off briefly then re-poll, unless cancelled.
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                        _ = cancel.cancelled() => return,
                    }
                }
            }
        }
    }

    /// Translate a failed `process` into the queue action: a retryable `AppError` -> `fail(true)`
    /// (backoff retry then dead-letter at the cap, SA-QUEUE-02); a terminal one -> `fail(false)`
    /// (immediate dead-letter). A job about to be parked is redacted first (SA-QUEUE-03); if that
    /// fails `fail` still runs, and the queue removes the text instead.
    pub async fn fail_job(&self, job: &WorkJob, err: &AppError) {
        let retryable = is_retryable(err);
        if !retryable || job.attempts >= self.cfg.max_attempts {
            self.redact_for_parking(job).await;
        }
        tracing::warn!(
            target: "memory",
            job_id = %job.id,
            attempts = job.attempts,
            retryable,
            error = %err,
            "write.job.failed"
        );
        if let Err(qe) = self
            .storage
            .fail(&job.scope.tenant, &job.id, retryable)
            .await
        {
            // A queue failure during fail-handling is logged; the lease will expire and the reaper
            // (C2) returns the job to pending (SA-QUEUE-02).
            tracing::error!(target: "memory", job_id = %job.id, error = %qe, "write.fail.queue_error");
        }
    }

    /// Store the job's payload redacted, as step 6 redacts a fact, marked so the queue parks it whole.
    // spec: phase-1-analysis#SA-QUEUE-03
    // A job going back for a retry is left alone: its names must reach step 6′ unredacted to be
    // screened, and it is no more lasting than a job waiting in the queue.
    async fn redact_for_parking(&self, job: &WorkJob) {
        let Json::Object(mut payload) = job.payload.clone() else {
            return;
        };
        let mut text = serde_json::Map::new();
        for field in ["content", "entities", "links", "source"] {
            if let Some(v) = payload.remove(field) {
                text.insert(field.to_string(), v);
            }
        }
        let clean = match self.pii_scan(&Json::Object(text)).await {
            Ok((Json::Object(clean), _)) => clean,
            Ok(_) => return,
            Err(e) => {
                tracing::warn!(target: "memory", job_id = %job.id, error = %e, "write.redact.failed");
                return;
            }
        };
        payload.extend(clean);
        payload.insert(crate::queue::REDACTED_KEY.to_string(), Json::Bool(true));
        if let Err(e) = self
            .storage
            .redact_job(&job.scope.tenant, &job.id, &Json::Object(payload))
            .await
        {
            tracing::warn!(target: "memory", job_id = %job.id, error = %e, "write.redact.failed");
        }
    }

    /// Build the internal `ScopeContext` from the job's `ScopeRef` (C4 *Shared Context*): single-team
    /// membership, `allowed_ops = { read, write, !forget }`, correlation id from the job's scope.
    fn ctx_from_job(&self, job: &WorkJob) -> ScopeContext {
        let teams = match &job.scope.team {
            Some(t) => vec![t.clone()],
            None => vec![],
        };
        ScopeContext {
            tenant: job.scope.tenant.clone(),
            teams,
            user: job.scope.user.clone(),
            token_jti: String::new(),
            allowed_ops: OpSet {
                read: true,
                write: true,
                forget: false,
            },
            // The job was already judged at the service layer before it was enqueued — the
            // fact-class authority check runs on the way in, not on the way out. Reconstructing an
            // authority claim here would re-decide a settled question with less information than
            // the original token carried.
            client: None,
            fact_authority: None,
            correlation_id: job.id.clone(),
        }
    }

    /// Process exactly one claimed job to a terminal queue state (complete or fail). Returns the
    /// outcome for metrics/tests. Bounded by `cfg.per_job_budget`.
    pub async fn process(
        &self,
        ctx: &ScopeContext,
        job: &WorkJob,
    ) -> Result<WriteOutcome, AppError> {
        match tokio::time::timeout(self.cfg.per_job_budget, self.process_inner(ctx, job)).await {
            Ok(r) => r,
            // A per-job budget overrun is a retryable store/provider stall.
            Err(_) => Err(AppError::Store(StoreError::Timeout)),
        }
    }

    async fn process_inner(
        &self,
        ctx: &ScopeContext,
        job: &WorkJob,
    ) -> Result<WriteOutcome, AppError> {
        // Step 0 — validate the job payload.
        if job.kind != JobKind::ExtractFact {
            return Err(AppError::Validation(
                ValidationKind::InvalidBody,
                "wrong job kind for the write pipeline".into(),
            ));
        }
        let payload: JobPayload = serde_json::from_value(job.payload.clone()).map_err(|e| {
            AppError::Validation(ValidationKind::InvalidBody, format!("payload: {e}"))
        })?;
        if !payload.content.is_object() {
            return Err(AppError::Validation(
                ValidationKind::InvalidBody,
                "content must be a JSON object".into(),
            ));
        }
        // A label breaking SA-COLL-01 is a malformed payload, dead-lettered like any other. C9
        // validated it on the way in, so this is a defence only (ADR-020).
        // spec: components/memory-store#SA-COLL-01
        let collection = payload.collection.clone().unwrap_or_default();
        if !crate::types::domain::is_valid_collection(&collection) {
            tracing::warn!(target: "memory", job_id = %job.id, "write.payload_invalid");
            return Err(AppError::Validation(
                ValidationKind::InvalidBody,
                "payload: collection".into(),
            ));
        }
        // The ADR-021 fields' bounds were validated by C9; a breach here is a malformed payload and
        // dead-letters the job (defence only).
        if !assertions_within_bounds(&payload) {
            tracing::warn!(target: "memory", job_id = %job.id, "write.payload_invalid");
            return Err(AppError::Validation(
                ValidationKind::InvalidBody,
                "payload: entities, links or derived_from".into(),
            ));
        }
        // Step 0 (ADR-022): a write the caller does not class a passage carries one triple; C9 checked
        // it, so anything else is a malformed payload (defence only).
        // spec: components/write-pipeline#SA-TRIPLE-11
        let passage = payload.fact_class.as_deref() == Some(self.cfg.passage_class.as_str());
        let triple = if passage {
            None
        } else {
            match crate::triple::check_triple(&payload.content) {
                Ok(t) => Some(t),
                Err(field) => {
                    tracing::warn!(target: "memory", job_id = %job.id, "write.payload_invalid");
                    return Err(AppError::Validation(
                        ValidationKind::InvalidBody,
                        format!("payload: {field}"),
                    ));
                }
            }
        };
        // Step 2′ (SA-TRIPLE-04): an entity object asserts the link subject –predicate→ object, unless
        // an equal link is already stated, which is then taken as the implied link with its own
        // confidence. Added after the bounds check, so it never counts against the caller's 16.
        // spec: components/write-pipeline#SA-TRIPLE-04
        let mut links = payload.links.clone();
        if let Some(t) = &triple {
            if let TripleObject::Entity { name } = &t.object {
                if !links.iter().any(|l| is_implied_link(l, t)) {
                    links.push(LinkAssertion {
                        from: t.subject.clone(),
                        kind: t.predicate.clone(),
                        to: name.clone(),
                        ended: payload.ended,
                        confidence: payload.confidence,
                    });
                }
            }
        }
        let assertions = Assertions {
            entities: payload.entities.clone(),
            links,
            derived_from: payload.derived_from.clone(),
            origin_teams: payload.origin_teams.clone(),
            author: payload.author.clone(),
            triple,
            single_valued: payload.single_valued,
            changed: payload.changed,
            ended: payload.ended,
            confidence: payload.confidence,
            valid_from: payload.valid_from,
        };
        tracing::info!(
            target: "memory",
            job_id = %job.id,
            tenant = %ctx.tenant,
            attempts = job.attempts,
            "write.job.claimed"
        );

        // A job an erasure removed since it was claimed must not write.
        if !self.job_live(job).await? {
            tracing::info!(target: "memory", job_id = %job.id, "write.erased_in_flight");
            return Ok(WriteOutcome::Erased);
        }

        // Step 0′ (SA-TRIPLE-14): a stated start later than the job's creation (C9 refused it first;
        // defence only) writes nothing.
        // spec: components/write-pipeline#SA-TRIPLE-14
        if payload.valid_from.is_some_and(|at| at > job.created_at) {
            tracing::info!(target: "memory", job_id = %job.id, "write.valid_from_future");
            self.complete(&job.scope.tenant, &job.id).await?;
            return Ok(WriteOutcome::Rejected);
        }

        // Step 1 — filter noise.
        if self.filter_noise(&payload.content) {
            tracing::info!(target: "memory", job_id = %job.id, "write.filtered_noise");
            self.complete(&job.scope.tenant, &job.id).await?;
            return Ok(WriteOutcome::FilteredNoise);
        }

        // Step 2 — intake: wrap the structured, agent-asserted content as a Fact (no LLM; ADR-015).
        // Each entity assertion becomes a further mention (ADR-021).
        let mut facts = self.intake(&payload.content, payload.memory_class);
        for f in &mut facts {
            // Step 2 (ADR-022): a triple's mentions are its subject and an entity object's name.
            // spec: components/write-pipeline#SA-TRIPLE-04
            if let Some(name) = assertions
                .triple
                .as_ref()
                .and_then(crate::triple::object_name)
            {
                f.entities.push(EntityMention {
                    surface: name.to_string(),
                    canonical_name: None,
                });
            }
            add_asserted_mentions(&mut f.entities, &assertions.entities);
        }
        tracing::info!(target: "memory", job_id = %job.id, fact_count = facts.len(), "write.intake");
        if facts.is_empty() {
            self.complete(&job.scope.tenant, &job.id).await?;
            return Ok(WriteOutcome::FilteredNoise);
        }

        // Steps 3-8 run once per ExtractedFact. The aggregate outcome reported is the "strongest" one
        // (Persisted > Quarantined > Rejected > FilteredNoise) so a multi-fact job's metric reflects
        // that at least one fact was admitted. A failure on any fact aborts the whole job (retryable);
        // persistence is idempotent on the derived fact id so a replay reuses the same id (step 8).
        let mut best = WriteOutcome::FilteredNoise;
        let source_id = self
            .upsert_source(ctx, payload.source.as_ref(), &collection)
            .await?;
        // spec: phase-1-analysis#SA-WGATE-02
        let source_trust = match payload.gate_trust {
            Some(trust) => trust,
            None => self.read_source_trust(ctx, source_id.as_deref()).await?,
        };

        for (idx, ef) in facts.into_iter().enumerate() {
            let outcome = self
                .process_fact(
                    ctx,
                    job,
                    ef,
                    idx,
                    source_id.clone(),
                    source_trust,
                    payload.fact_class.clone(),
                    payload.visibility.unwrap_or(Visibility::UserPrivate),
                    &collection,
                    &assertions,
                )
                .await?;
            if outcome == WriteOutcome::Erased {
                return Ok(outcome);
            }
            best = stronger(best, outcome);
        }

        self.complete(&job.scope.tenant, &job.id).await?;
        Ok(best)
    }

    /// Run steps 3-8 for one extracted fact, returning its terminal (non-completing) outcome. The job
    /// is completed by the caller once every fact has been processed.
    ///
    /// `fact_class` is the caller-owned class C9 already authorised; it is stored verbatim on the
    /// persisted fact, or on the quarantine record when the gate holds the write.
    #[allow(clippy::too_many_arguments)]
    async fn process_fact(
        &self,
        ctx: &ScopeContext,
        job: &WorkJob,
        ef: ExtractedFact,
        fact_index: usize,
        source_id: Option<String>,
        source_trust: f64,
        // `fact_class` is carried from the request rather than derived from the content: it is what
        // the caller *claimed*, not something extracted, which is why it is a parameter here and not
        // a field on `ExtractedFact` (and why `intake`'s public signature is unchanged).
        fact_class: Option<String>,
        visibility: Visibility,
        collection: &str,
        assertions: &Assertions,
    ) -> Result<WriteOutcome, AppError> {
        // Step 3 — normalise.
        let mut ef = self.normalise(&ef);

        // Steps 6′ and 7′ over the stated names and kinds, ahead of step 4 so that a name the scan
        // drops is never written and an instruction-like name or kind writes nothing at all.
        // spec: components/memory-store#SA-GRAPH-01
        let mut assertions = assertions.clone();
        let names_review = self.screen_names(&mut ef, &mut assertions).await?;
        if names_review.instruction_like {
            tracing::info!(target: "memory", job_id = %job.id, decision = ?GateDecision::Reject, "write.gate");
            return Ok(WriteOutcome::Rejected);
        }

        // Steps 4 and 4′ run after the gate, on the admit path: earlier, they would write an entity
        // for a write the gate then rejects or holds, and the name of an unscreened value.

        // Step 5 — score. A stated confidence is the fact's as stated, and the gate reads it
        // (SA-TRIPLE-14); without one, the structured write scores as before.
        // spec: components/write-pipeline#SA-TRIPLE-14
        let (salience, confidence) = self.score(&ef, source_trust);
        let confidence = assertions.confidence.unwrap_or(confidence);

        // Step 6 — PII scan (redact in place, or flag for review).
        let (content, mut pii_review) = self.pii_scan(&ef.content).await?;
        pii_review |= names_review.review;

        // Step 6″ (ADR-022): a triple whose subject or entity object the scan dropped or redacted, or
        // whose number, money or date value it changed, states nothing checkable.
        // spec: components/write-pipeline#SA-TRIPLE-02
        if let Some(t) = &assertions.triple {
            if triple_redacted(t, &ef, &content) {
                tracing::info!(target: "memory", job_id = %job.id, "write.triple_redacted");
                return Ok(WriteOutcome::Rejected);
            }
        }

        // Step 7 — write gate (instruction detector then trust bands).
        let instr = self.detect_instruction(&content);
        let decision = self.write_gate(confidence, source_trust, instr);
        let trust = self.trust_score(confidence, source_trust, instr);
        tracing::info!(
            target: "memory",
            job_id = %job.id,
            decision = ?decision,
            trust,
            is_instruction_like = instr.is_instruction_like,
            "write.gate"
        );

        // Step 8 (addition) — a consolidated write's origins are re-checked under the job's scope: one
        // absent or ended since intake rejects the write.
        // Whatever the gate decided, so a queued summary of erased facts is not held either.
        let mut derived_from = assertions.derived_from.clone();
        derived_from.sort();
        derived_from.dedup();
        // Intake reads the origins under all the caller's teams; the job's scope holds one of them, so
        // the origins are read again under the teams intake used.
        let origin_ctx = assertions.origin_ctx(ctx);
        if !self.origins_live(&origin_ctx, &derived_from).await? {
            // A replay also takes back what an earlier attempt stored before it failed.
            let id = self.derive_fact_id(&job.scope, job.idempotency_key.as_deref(), fact_index);
            self.take_back_fact(ctx, job, &id).await?;
            tracing::info!(target: "memory", job_id = %job.id, "write.derived_from_stale");
            return Ok(WriteOutcome::Rejected);
        }

        match decision {
            GateDecision::Reject => Ok(WriteOutcome::Rejected),
            GateDecision::Quarantine => {
                // A held triple write ends nothing and records no conflict (ADR-022).
                // SPEC-GAP-2: write-pipeline.md (amendment ADR-022) says "the held record keeps the
                // flags in its payload for a reviewer", but `QuarantineRecord` has no payload and
                // `0011_triples` declares no field for `single_valued`, `changed` or `ended` on the
                // SCHEMAFULL `quarantine` table. The content (the triple) is kept; the flags are not,
                // so a released held write is stored without them.
                let qr = QuarantineRecord {
                    id: format!("quarantine:{}", Uuid::now_v7()),
                    fact_id: Some(self.derive_fact_id(
                        &job.scope,
                        job.idempotency_key.as_deref(),
                        fact_index,
                    )),
                    content,
                    // No entity exists for a held write; `entities_asserted` keeps what it stated.
                    entities: Vec::new(),
                    source_id,
                    memory_class: ef.memory_class,
                    fact_class,
                    owner: job.scope.clone(),
                    trust_score: trust,
                    confidence,
                    salience,
                    is_instruction_like: instr.is_instruction_like,
                    pii_review,
                    idempotency_key: job.idempotency_key.clone(),
                    reason: "trust in [trust_quarantine, trust_admit)".into(),
                    quarantined_at: Utc::now(),
                    collection: collection.to_string(),
                    // No link is written for a held write; the record keeps what it asserted.
                    entities_asserted: assertions.entities.clone(),
                    links_asserted: assertions.links.clone(),
                    derived_from: assertions.derived_from.clone(),
                };
                self.quarantine(ctx, &qr).await?;
                if !self.job_live(job).await? {
                    self.storage.drop_quarantine(&qr).await?;
                    tracing::info!(target: "memory", job_id = %job.id, "write.erased_in_flight");
                    return Ok(WriteOutcome::Erased);
                }
                Ok(WriteOutcome::Quarantined)
            }
            GateDecision::Admit => {
                // Step 4 — entity-resolve (rules -> ML -> create-new), over the redacted content. A new
                // entity takes the write's collection; an existing one keeps its own (ADR-020). Step 4′
                // adds the assertions' aliases.
                ef.content = content.clone();
                let resolved = self
                    .resolve_mentions(ctx, &ef, collection, visibility)
                    .await?;
                let created = resolved.created;
                let by_name = alias_names(&assertions.entities, resolved.by_name);

                // Step 4″ (ADR-022): the triple's indexed parts, by the entities it resolved to.
                // spec: phase-1-analysis#SA-TRIPLE-01
                let triple_key = match &assertions.triple {
                    Some(t) => match triple_key_of(t, &by_name) {
                        Some(k) => Some(TripleKey {
                            author: assertions.author.clone(),
                            ..k
                        }),
                        None => {
                            // Every mention resolves (step 4), so this is a defence only.
                            tracing::warn!(target: "memory", job_id = %job.id, "write.payload_invalid");
                            return Err(AppError::Validation(
                                ValidationKind::InvalidBody,
                                "payload: triple entities".into(),
                            ));
                        }
                    },
                    None => None,
                };

                // Step 8 — embed and persist. `valid_from` is the writer's start when stated
                // (SA-TRIPLE-14); `ingested_at` stays the time of the write.
                let now = Utc::now();
                let valid_from = assertions.valid_from.unwrap_or(now);
                let fact = Fact {
                    id: self.derive_fact_id(&job.scope, job.idempotency_key.as_deref(), fact_index),
                    content,
                    entities: resolved.ids,
                    source_id,
                    memory_class: ef.memory_class,
                    // The class of *this* write. A superseding write carries its own, never its
                    // predecessor's: a supersession is a new assertion that passed the gate with its
                    // own token, so inheriting would attach a label no gate judged for it.
                    // spec: phase-1-analysis#SA-AUTHZ-10
                    fact_class,
                    visibility,
                    owner: job.scope.clone(),
                    valid_from,
                    valid_to: None,
                    ingested_at: now,
                    confidence,
                    salience,
                    stability: 1.0,
                    pii_review,
                    supersedes: None,
                    superseded_by: None,
                    derived_from,
                    last_recalled_at: None,
                    collection: collection.to_string(),
                    single_valued: assertions.single_valued,
                    changed: assertions.changed,
                    triple_key,
                };
                // The link is written first, so a job that fails after the fact is stored can be
                // found by a delete of that fact.
                self.storage
                    .link_job_fact(&job.scope, &job.id, &fact.id)
                    .await?;
                self.embed_and_persist(ctx, &fact).await?;
                // Step 8′ — the links, admit path only.
                self.write_links(
                    ctx,
                    job,
                    &fact,
                    &assertions.links,
                    assertions.author.as_deref(),
                    &by_name,
                    assertions.triple.as_ref(),
                )
                .await?;
                // Step 8″ (ADR-022) — change, end and conflict, after the fact's own links so a link
                // it repeats stays live.
                self.change_end_or_conflict(ctx, job, &fact, &assertions)
                    .await?;

                // An erasure deletes the job before it reads what to erase, so either it saw this
                // write, or this read sees the job gone and the write is taken back.
                // Links closed by the write are not restored.
                if !self.job_live(job).await? {
                    self.undo_write(ctx, &fact, &created).await;
                    tracing::info!(target: "memory", job_id = %job.id, "write.erased_in_flight");
                    return Ok(WriteOutcome::Erased);
                }
                // An origin erased during the embedding leaves no job to see and no stored summary
                // for its cascade to find, so the origins are read again. Only the fact goes:
                // an origin can end without any erasure, so `undo_write` would erase the writer's other
                // pending work. An erasure whose cascade reads before this row commits and whose delete
                // commits after this read is still missed on SurrealDB and PostgreSQL (FU-058).
                if !self.origins_live(&origin_ctx, &fact.derived_from).await? {
                    self.take_back_fact(ctx, job, &fact.id).await?;
                    tracing::info!(target: "memory", job_id = %job.id, "write.derived_from_stale");
                    return Ok(WriteOutcome::Rejected);
                }
                // Step 4′: stored last, so a write rejected after persist leaves no name on the
                // entity. A failure fails the job and the replay repeats them.
                self.add_aliases(
                    ctx,
                    &assertions.entities,
                    &by_name,
                    visibility,
                    job.scope.team.as_deref(),
                    collection,
                )
                .await?;
                tracing::info!(
                    target: "memory",
                    job_id = %job.id,
                    fact_id = %fact.id,
                    memory_class = ?fact.memory_class,
                    confidence,
                    salience,
                    pii_review,
                    "write.persisted"
                );
                Ok(WriteOutcome::Persisted)
            }
        }
    }

    /// Step 8″ (ADR-022; admit path): for a triple written `single_valued` or `ended`, read the live
    /// facts of the owner with the same subject and predicate — every visibility and collection — and
    /// end the earlier value (a stated change), end the same triple (a stated end), or record a
    /// conflict with each disagreeing fact. Nothing is decided on a judgement of memory's own. A
    /// replay ends nothing twice and records nothing twice.
    // spec: components/write-pipeline#SA-TRIPLE-06
    // spec: components/write-pipeline#SA-TRIPLE-07
    // spec: components/write-pipeline#SA-TRIPLE-08
    async fn change_end_or_conflict(
        &self,
        ctx: &ScopeContext,
        job: &WorkJob,
        fact: &Fact,
        a: &Assertions,
    ) -> Result<(), AppError> {
        let Some(key) = &fact.triple_key else {
            return Ok(());
        };
        if !a.single_valued && !a.ended {
            return Ok(());
        }
        let live: Vec<Fact> = self
            .storage
            .live_same_subject_predicate(
                ctx,
                &fact.owner,
                &key.subject_entity,
                &key.predicate,
                crate::store::graph::SAME_SUBJECT_PREDICATE_MAX,
            )
            .await?
            .into_iter()
            .filter(|f| f.id != fact.id)
            .collect();
        let same_object = |f: &Fact| {
            f.triple_key.as_ref().is_some_and(|k| {
                k.object_kind == key.object_kind
                    && k.object_value == key.object_value
                    && k.object_unit == key.object_unit
            })
        };
        // A change or an end is stated for the writer's own visibility and collection (ADR-022 (3)),
        // and the owner alone changes its facts (SA-AUTHZ-07). Among the owner's callers, a write
        // with an author ends only facts of that author; one with none ends any (step 8″.6).
        // spec: phase-1-analysis#SA-TRIPLE-15
        let mine = |f: &Fact| {
            a.author.is_none()
                || f.triple_key.as_ref().and_then(|k| k.author.as_ref()) == a.author.as_ref()
        };
        let own_place = |f: &Fact| {
            f.visibility == fact.visibility
                && f.collection == fact.collection
                && f.owner.user == ctx.user
                && mine(f)
        };
        if a.ended {
            let matched: Vec<&Fact> = live
                .iter()
                .filter(|f| own_place(f) && same_object(f))
                .take(CHANGE_MAX)
                .collect();
            let not_ours = live.iter().filter(|f| same_object(f) && !mine(f)).count();
            if not_ours > 0 {
                tracing::info!(target: "memory", job_id = %job.id, not_ours, "write.fact_not_ours");
            }
            self.end_earlier(ctx, fact, &matched).await?;
            // SA-TRIPLE-07: the new fact is kept as the evidence that the triple stopped.
            self.storage
                .end_validity(ctx, &fact.id, fact.valid_from)
                .await?;
            tracing::info!(target: "memory", job_id = %job.id, matched = matched.len(), "write.triple_ended");
            return Ok(());
        }
        let differing: Vec<&Fact> = live.iter().filter(|f| !same_object(f)).collect();
        let mut conflicting: Vec<&Fact> = Vec::new();
        if a.changed {
            let (own, other): (Vec<&Fact>, Vec<&Fact>) =
                differing.into_iter().partition(|f| own_place(f));
            let ended: Vec<&Fact> = own.iter().copied().take(CHANGE_MAX).collect();
            self.end_earlier(ctx, fact, &ended).await?;
            tracing::info!(target: "memory", job_id = %job.id, ended = ended.len(), "write.triple_changed");
            // Beyond 16, and every disagreeing fact of another visibility or collection, conflict.
            conflicting.extend(own.into_iter().skip(CHANGE_MAX));
            conflicting.extend(other);
        } else {
            conflicting = differing;
        }
        let mut recorded = 0u32;
        let now = Utc::now();
        for f in conflicting.into_iter().take(CHANGE_MAX) {
            if self
                .storage
                .record_conflict(ctx, &fact.id, &f.id, now)
                .await?
            {
                recorded += 1;
            }
        }
        if !a.changed {
            tracing::info!(target: "memory", job_id = %job.id, recorded, "write.conflicts");
        }
        Ok(())
    }

    /// End each of `facts` (newest first) at the later of the new fact's `valid_from` and its own; the
    /// most recent is superseded by the new fact instead (SA-TRIPLE-06, SA-TRIPLE-14).
    async fn end_earlier(
        &self,
        ctx: &ScopeContext,
        new: &Fact,
        facts: &[&Fact],
    ) -> Result<(), AppError> {
        for (i, f) in facts.iter().enumerate() {
            let at = new.valid_from.max(f.valid_from);
            if i == 0 {
                self.storage.supersede(ctx, &f.id, &new.id, at).await?;
            } else {
                self.storage.end_validity(ctx, &f.id, at).await?;
            }
        }
        Ok(())
    }

    /// Whether the job's row still exists (an erasure removes the jobs it covers first).
    async fn job_live(&self, job: &WorkJob) -> Result<bool, AppError> {
        self.storage
            .job_is_live(&job.scope.tenant, &job.id)
            .await
            .map_err(AppError::Queue)
    }

    /// Whether every fact a summary is derived from is still present and has not ended.
    async fn origins_live(&self, ctx: &ScopeContext, ids: &[String]) -> Result<bool, AppError> {
        for id in ids {
            let fact = self.storage.get_fact(ctx, id).await?;
            if fact.is_none_or(|f| f.valid_to.is_some()) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Remove a summary whose origins have gone, if it was stored. A failure fails the job, so a retry
    /// takes it back. The job's links go first: `hard_delete` would otherwise remove this job with its
    /// fact.
    async fn take_back_fact(
        &self,
        ctx: &ScopeContext,
        job: &WorkJob,
        id: &str,
    ) -> Result<(), AppError> {
        self.storage
            .unlink_job_facts(&job.scope.tenant, &job.id)
            .await?;
        match self.storage.hard_delete(ctx, id).await {
            Ok(_) | Err(StoreError::NotFound) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Take back a persisted write an erasure overtook: the fact with its links, then the entities it
    /// created. A failure is logged: the erasure has already been reported. `hard_delete` has already
    /// taken a created entity nothing else uses.
    async fn undo_write(&self, ctx: &ScopeContext, fact: &Fact, created: &[String]) {
        if let Err(e) = self.storage.hard_delete(ctx, &fact.id).await {
            tracing::error!(target: "memory", fact_id = %fact.id, error = %e, "write.undo_failed");
        }
        for id in created {
            match self.storage.erase_entity(ctx, id).await {
                Ok(_) | Err(StoreError::NotFound) => {}
                Err(e) => {
                    tracing::error!(target: "memory", entity_id = %id, error = %e, "write.undo_failed");
                }
            }
        }
    }

    // ---- step functions (ordered) ----

    /// Step 1. `true` => content is empty or low-signal and the job is completed as noise. Drops when,
    /// after trimming, the serialised content is an empty object/array or the total non-whitespace
    /// string length across all string leaves is `< 3` characters (data minimisation, HLD 06).
    pub fn filter_noise(&self, content: &Json) -> bool {
        match content {
            Json::Object(m) if m.is_empty() => return true,
            Json::Array(a) if a.is_empty() => return true,
            _ => {}
        }
        non_ws_string_len(content) < 3
    }

    /// Step 2. Intake the structured, agent-asserted `content` as a single `ExtractedFact`
    /// (`confidence = 1.0`, `memory_class` from the request or `Episodic`); recall performs no LLM
    /// extraction (ADR-015). The content is treated as data, never instructions. Returns a one-element
    /// vector (the downstream steps run per `ExtractedFact`, so the shape is preserved).
    pub fn intake(&self, content: &Json, memory_class: Option<MemoryClass>) -> Vec<ExtractedFact> {
        let entities = scan_entity_mentions(content);
        vec![ExtractedFact {
            content: content.clone(),
            entities,
            memory_class: memory_class.unwrap_or(MemoryClass::Episodic),
            confidence: 1.0,
        }]
    }

    /// Step 3. Canonicalise content: sort object keys, collapse internal whitespace runs to single
    /// spaces in string values, and apply Unicode NFC normalisation. Pure; no dependency.
    pub fn normalise(&self, ef: &ExtractedFact) -> ExtractedFact {
        ExtractedFact {
            content: normalise_json(&ef.content),
            entities: ef
                .entities
                .iter()
                .map(|m| EntityMention {
                    surface: collapse_ws(&m.surface),
                    canonical_name: m.canonical_name.clone(),
                })
                .collect(),
            memory_class: ef.memory_class,
            confidence: ef.confidence,
        }
    }

    /// Step 4. Resolve each mention to an entity id via the rules -> ML -> create-new ladder (v1),
    /// returning `>=1` entity ids. Never silently overwrites an existing entity (HLD 04); on ambiguity
    /// a new entity is created via `MemoryStore::put_entity` rather than risking a wrong merge.
    pub async fn resolve_entities(
        &self,
        ctx: &ScopeContext,
        ef: &ExtractedFact,
    ) -> Result<Vec<String>, AppError> {
        Ok(self
            .resolve_mentions(ctx, ef, "", Visibility::UserPrivate)
            .await?
            .ids)
    }

    /// Step 4 for a write into `collection`, with the names it resolved: an entity the ladder creates
    /// takes `collection`; an entity found by name keeps its own (ADR-020).
    // `resolve_entities` keeps its `Vec<String>`; this crate-visible step returns `ResolvedEntities`.
    pub(crate) async fn resolve_mentions(
        &self,
        ctx: &ScopeContext,
        ef: &ExtractedFact,
        collection: &str,
        visibility: Visibility,
    ) -> Result<ResolvedEntities, AppError> {
        let mut ids: Vec<String> = Vec::new();
        let mut by_name: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut created: Vec<String> = Vec::new();
        // A shared write must not land on an entity through a name only a private fact gave it: the
        // entity's other names would then reach the write's readers.
        let reachable = |e: &Entity, name: &str| {
            visibility == Visibility::UserPrivate
                || !e
                    .private_aliases
                    .iter()
                    .any(|a| a == name || normalise_name(a) == normalise_name(name))
        };

        for mention in &ef.entities {
            let normalised = normalise_surface(&mention.surface);
            if normalised.is_empty() {
                continue;
            }
            if let Some(id) = by_name.get(&normalised) {
                push_unique(&mut ids, id.clone());
                continue;
            }

            // (a) Rules tier: exact canonical-name or alias match within scope.
            let mut hits = self.storage.find_entity_by_name(ctx, &normalised).await?;
            hits.retain(|e| reachable(e, &normalised));
            if hits.len() == 1 {
                by_name.insert(normalised, hits[0].id.clone());
                push_unique(&mut ids, hits[0].id.clone());
                continue;
            }
            if hits.len() > 1 {
                // Ambiguous exact match: pick the lexicographically-smallest id deterministically
                // rather than guess; a later maintenance merge folds duplicates (C7).
                let mut sorted = hits;
                sorted.sort_by(|a, b| a.id.cmp(&b.id));
                by_name.insert(normalised, sorted[0].id.clone());
                push_unique(&mut ids, sorted[0].id.clone());
                continue;
            }

            // (b) ML tier: string-similarity against in-scope candidates. With no exact hit and the v1
            // store exposing no broad entity scan, the candidate set for similarity is the rules-tier
            // lookup against the *raw* surface form (a case/punctuation variant). A single candidate at
            // or above the admit threshold resolves; otherwise escalate to create-new.
            let mut raw_hits = self
                .storage
                .find_entity_by_name(ctx, &mention.surface)
                .await?;
            raw_hits.retain(|e| reachable(e, &mention.surface));
            if let Some(best) = best_similarity_match(&normalised, &raw_hits) {
                by_name.insert(normalised, best.clone());
                push_unique(&mut ids, best);
                continue;
            }

            // (c) create-new tier (v1): create a NEW entity (never merge into an existing one without an
            // above-threshold match, never silently overwrite). Duplicate entities created this way are
            // folded later by the C7 Maintenance Worker via `merge_entities`.
            // The entity keeps its team even from a private fact: team and folder erasure find rows
            // by it, and the read filter, not the owner, decides who sees the name.
            let entity = Entity {
                id: format!("entity:{}", Uuid::now_v7()),
                canonical_name: normalised.clone(),
                aliases: vec![mention.surface.clone()],
                owner: ScopeRef {
                    tenant: ctx.tenant.clone(),
                    team: ctx.teams.first().cloned(),
                    user: ctx.user.clone(),
                },
                collection: collection.to_string(),
                private_aliases: vec![],
                alias_folders: vec![],
            };
            self.storage.put_entity(&entity).await?;
            created.push(entity.id.clone());
            by_name.insert(normalised, entity.id.clone());
            push_unique(&mut ids, entity.id);
        }

        if ids.is_empty() {
            // A fact must connect >=1 entity (2C.2). When the extractor offered no usable mention,
            // synthesise a single entity from the content so the fact is still anchored rather than
            // dropping a salient assertion.
            let synth = synth_entity_name(&ef.content);
            let entity = Entity {
                id: format!("entity:{}", Uuid::now_v7()),
                canonical_name: synth.clone(),
                aliases: vec![],
                owner: ScopeRef {
                    tenant: ctx.tenant.clone(),
                    team: ctx.teams.first().cloned(),
                    user: ctx.user.clone(),
                },
                collection: collection.to_string(),
                private_aliases: vec![],
                alias_folders: vec![],
            };
            if synth.is_empty() {
                return Err(AppError::Validation(
                    ValidationKind::InvalidBody,
                    "a fact must connect at least one entity".into(),
                ));
            }
            self.storage.put_entity(&entity).await?;
            created.push(entity.id.clone());
            ids.push(entity.id);
        }

        tracing::info!(
            target: "memory",
            entity_count = ids.len(),
            created_count = created.len(),
            "write.entities.resolved"
        );
        Ok(ResolvedEntities {
            ids,
            by_name,
            created,
        })
    }

    /// Steps 6′ and 7′ over the stated names and kinds: the personal-data scan drops a name (with its
    /// mention and every link naming it) or a kind (with its link) holding a span at
    /// `pii_redact_conf` or above, and a lower-confidence span flags the fact for review; a known
    /// instruction phrase makes the whole write instruction-like. A leading verb does not: it is
    /// ordinary in a name or a kind ("run by"). The names step 2 took from `content.subject` and
    /// `content.object` are screened with them.
    // A person's name in `content.subject` would otherwise become an entity name while step 6 redacts
    // it from the fact. Such a mention is dropped with its links; the fact keeps the redacted content.
    async fn screen_names(
        &self,
        ef: &mut ExtractedFact,
        a: &mut Assertions,
    ) -> Result<NamesReview, AppError> {
        let mut names: Vec<String> = Vec::new();
        for e in &a.entities {
            names.push(e.name.clone());
            names.extend(e.aliases.iter().cloned());
        }
        names.extend(ef.entities.iter().map(|m| m.surface.clone()));
        if names.is_empty() && a.links.is_empty() {
            return Ok(NamesReview::default());
        }
        let kinds: Vec<String> = a.links.iter().map(|l| l.kind.clone()).collect();
        let screened = serde_json::json!({ "names": names, "kinds": kinds });
        let mut phrases = 0u32;
        for_each_string(&screened, &mut |s| {
            phrases += count_name_phrases(&crate::text::normalise_kind(s));
        });
        let instruction_like = phrases >= 1;
        let mut review = false;
        let mut bad_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut bad_kinds: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for span in self.pii.scan(&screened).await? {
            if span.confidence < self.cfg.pii_redact_conf {
                review = true;
                continue;
            }
            let mut parts = span.json_pointer.trim_start_matches('/').splitn(2, '/');
            match (
                parts.next(),
                parts.next().and_then(|i| i.parse::<usize>().ok()),
            ) {
                (Some("names"), Some(i)) => {
                    if let Some(n) = names.get(i) {
                        bad_names.insert(normalise_name(n));
                    }
                }
                (Some("kinds"), Some(i)) => {
                    bad_kinds.insert(i);
                }
                _ => review = true,
            }
        }
        let (mut names_dropped, mut links_dropped) = (0u32, 0u32);
        if !bad_names.is_empty() || !bad_kinds.is_empty() {
            // A dropped name takes its whole assertion: its other names would still resolve to the
            // entity the dropped name identified.
            let before = a.entities.len();
            let mut gone: std::collections::HashSet<String> = std::collections::HashSet::new();
            a.entities.retain(|e| {
                let all: Vec<String> = std::iter::once(&e.name)
                    .chain(e.aliases.iter())
                    .map(|n| normalise_name(n))
                    .collect();
                if all.iter().any(|n| bad_names.contains(n)) {
                    gone.extend(all);
                    false
                } else {
                    true
                }
            });
            names_dropped = (before - a.entities.len()) as u32;
            gone.extend(bad_names.iter().cloned());
            ef.entities
                .retain(|m| !gone.contains(&normalise_name(&m.surface)));
            let before = a.links.len();
            let mut i = 0usize;
            a.links.retain(|l| {
                let keep = !bad_kinds.contains(&i)
                    && !gone.contains(&normalise_name(&l.from))
                    && !gone.contains(&normalise_name(&l.to));
                i += 1;
                keep
            });
            links_dropped = (before - a.links.len()) as u32;
        }
        tracing::info!(
            target: "memory",
            names_dropped,
            links_dropped,
            "write.pii.scanned"
        );
        Ok(NamesReview {
            instruction_like,
            review,
        })
    }

    /// Step 4′: each assertion's aliases join its resolved entity — existing first, deduplicated by
    /// `normalise_name`, at most 32 — written only when the set changed. Returns `by_name` extended
    /// with every normalised alias of each assertion.
    // An alias from a private fact is stored as `Entity::private_aliases`, read by the entity's owner
    // alone, and a later shared fact stating it makes it public.
    // Only the entity's owner adds to it (F-002's rule for every `put_*`): another user's alias
    // resolves within the write and is not stored.
    // Each stated alias records the folder that stated it. An alias stored without a folder keeps
    // none, so no erasure takes it.
    async fn add_aliases(
        &self,
        ctx: &ScopeContext,
        asserted: &[EntityAssertion],
        by_name: &std::collections::HashMap<String, String>,
        visibility: Visibility,
        team: Option<&str>,
        collection: &str,
    ) -> Result<(), AppError> {
        let mut aliases_dropped = 0usize;
        for a in asserted {
            let Some(id) = by_name.get(&normalise_name(&a.name)).cloned() else {
                continue;
            };
            if a.aliases.is_empty() {
                continue;
            }
            let Some(mut entity) = self.storage.get_entity(ctx, &id).await? else {
                continue;
            };
            if entity.owner.user != ctx.user {
                aliases_dropped += a.aliases.len();
                continue;
            }
            let private = visibility == Visibility::UserPrivate;
            let mut seen: std::collections::HashSet<String> =
                entity.aliases.iter().map(|x| normalise_name(x)).collect();
            let mut merged = entity.aliases.clone();
            let mut held = entity.private_aliases.clone();
            let mut folders = entity.alias_folders.clone();
            let mut added = std::collections::HashSet::new();
            for alias in &a.aliases {
                let norm = normalise_name(alias);
                if seen.insert(norm.clone()) {
                    added.insert(norm);
                    merged.push(alias.clone());
                    if private {
                        held.push(alias.clone());
                    }
                } else if !private {
                    held.retain(|h| normalise_name(h) != norm);
                }
            }
            if merged.len() > 32 {
                aliases_dropped += merged.len() - 32;
                merged.truncate(32);
                held.retain(|h| merged.contains(h));
            }
            let stored: std::collections::HashSet<String> =
                merged.iter().map(|x| normalise_name(x)).collect();
            for alias in a.aliases.iter().map(|x| normalise_name(x)) {
                let known = folders.iter().any(|f| f.alias == alias);
                let folder = AliasFolder {
                    alias: alias.clone(),
                    team: team.map(str::to_string),
                    collection: collection.to_string(),
                };
                if stored.contains(&alias)
                    && (added.contains(&alias) || known)
                    && !folders.contains(&folder)
                {
                    folders.push(folder);
                }
            }
            if merged != entity.aliases
                || held != entity.private_aliases
                || folders != entity.alias_folders
            {
                entity.aliases = merged;
                entity.private_aliases = held;
                entity.alias_folders = folders;
                self.storage.put_entity(&entity).await?;
            }
        }
        if aliases_dropped > 0 {
            tracing::info!(target: "memory", aliases_dropped, "write.entities.resolved");
        }
        Ok(())
    }

    /// Step 8′ (admit path): write each link of the request, or close the live link an `ended`
    /// assertion names, recording the fact's role on it. Ids are deterministic, so a replayed job
    /// reaches the same relationships and rows.
    // spec: components/memory-store#SA-GRAPH-03
    #[allow(clippy::too_many_arguments)]
    async fn write_links(
        &self,
        ctx: &ScopeContext,
        job: &WorkJob,
        fact: &Fact,
        links: &[LinkAssertion],
        assertions_author: Option<&str>,
        by_name: &std::collections::HashMap<String, String>,
        triple: Option<&Triple>,
    ) -> Result<(), AppError> {
        if links.is_empty() {
            return Ok(());
        }
        let existing = self.storage.relationship_kinds(ctx, 1000).await?;
        let (mut written, mut ended, mut skipped) = (0u32, 0u32, 0u32);
        for (i, l) in links.iter().enumerate() {
            let (Some(from), Some(to)) = (
                by_name.get(&normalise_name(&l.from)),
                by_name.get(&normalise_name(&l.to)),
            ) else {
                skipped += 1;
                tracing::info!(target: "memory", job_id = %job.id, "write.link_skipped");
                continue;
            };
            // The implied link's kind is the predicate as stored, never lower-cased or merged
            // (SA-TRIPLE-04): the link's kind always equals the fact's predicate.
            // spec: components/write-pipeline#SA-TRIPLE-04
            let kind = match triple.filter(|t| is_implied_link(l, t)) {
                Some(t) => t.predicate.clone(),
                None => merge_kind(&l.kind, &existing),
            };
            let now = Utc::now();
            let live = self
                .storage
                .find_live_relationship(ctx, from, &kind, to)
                .await?;
            // The link's owner alone changes its row, and among an owner's callers its author. An
            // `ended` from anyone else closes nothing. A teammate's fact still counts as asserting the
            // link, so they can follow it, and closure reads only the owner's facts (SA-GRAPH-04). A
            // fact of the same owner counts there, so another author's does not assert: it could keep
            // the link alive past a closure or an erasure, or become its sole asserter.
            // spec: phase-1-analysis#SA-GRAPH-14
            let author = assertions_author;
            let may_change = |r: &Relationship| {
                r.owner.user == ctx.user && author.is_none_or(|a| r.author.as_deref() == Some(a))
            };
            if l.ended {
                match live.filter(|r| may_change(r)) {
                    Some(r) => {
                        self.storage
                            .end_relationship_validity(ctx, &r.id, now)
                            .await?;
                        self.storage
                            .link_fact(ctx, &r.id, &fact.id, LinkRole::Ended)
                            .await?;
                        ended += 1;
                    }
                    None => {
                        tracing::info!(target: "memory", job_id = %job.id, "write.link_end_unmatched");
                    }
                }
                continue;
            }
            let confidence = l.confidence.unwrap_or(1.0);
            let (rel, ours) = match live {
                Some(r) if r.owner.user == ctx.user && !may_change(&r) => {
                    tracing::info!(target: "memory", job_id = %job.id, "write.link_not_ours");
                    continue;
                }
                Some(mut r) => {
                    let ours = may_change(&r);
                    if ours {
                        r.confidence = confidence;
                    }
                    (r, ours)
                }
                None => (
                    Relationship {
                        id: format!(
                            "relationship:{}",
                            deterministic_uuid(&format!("relationship:{}:{i}", fact.id))
                        ),
                        kind,
                        from: from.clone(),
                        to: to.clone(),
                        valid_from: now,
                        valid_to: None,
                        ingested_at: now,
                        confidence,
                        source_id: fact.source_id.clone(),
                        owner: fact.owner.clone(),
                        collection: fact.collection.clone(),
                        replaces: None,
                        owner_only: false,
                        author: author.map(str::to_owned),
                    },
                    true,
                ),
            };
            if ours {
                self.storage.put_relationship(&rel).await?;
            }
            self.storage
                .link_fact(ctx, &rel.id, &fact.id, LinkRole::Asserted)
                .await?;
            written += 1;
        }
        tracing::info!(target: "memory", job_id = %job.id, written, ended, skipped, "write.links");
        Ok(())
    }

    /// Step 5. Score `salience` and `confidence`, each clamped to `[0,1]` (SA-SCORE-01).
    /// `confidence = clamp01(extractor_confidence * (0.5 + 0.5 * source_trust))`; `salience` is a
    /// bounded heuristic from content richness (entity-mention count, predicate presence). Pure.
    pub fn score(&self, ef: &ExtractedFact, source_trust: f64) -> (f64, f64) {
        let confidence = scaled_confidence(ef.confidence, source_trust);

        // Salience heuristic: base 0.3, +0.15 per entity mention (capped at 0.45 of mention weight),
        // +0.2 when the content names a predicate/relation field. Bounded to [0,1].
        let mention_weight = (ef.entities.len() as f64 * 0.15).min(0.45);
        let predicate_bonus = if has_predicate(&ef.content) { 0.2 } else { 0.0 };
        let salience = clamp01(0.3 + mention_weight + predicate_bonus);
        (salience, confidence)
    }

    /// Steps 6′ and 7′ over what a team import names: the phrase check and the personal-data scan of
    /// entity names and aliases, or of a link's kind. Writes nothing. An import has no assertion to
    /// drop part of, so a hit the write path would drop or reject refuses the item.
    // spec: components/memory-store#SA-GRAPH-01
    pub(crate) async fn screen_imported(&self, texts: &[&str]) -> Result<NameVerdict, AppError> {
        let screened = serde_json::json!({ "names": texts });
        let mut phrases = 0u32;
        for_each_string(&screened, &mut |s| {
            phrases += count_name_phrases(&crate::text::normalise_kind(s));
        });
        if phrases >= 1 {
            return Ok(NameVerdict::Refused);
        }
        let mut verdict = NameVerdict::Clean;
        for span in self.pii.scan(&screened).await? {
            if span.confidence >= self.cfg.pii_redact_conf {
                return Ok(NameVerdict::Refused);
            }
            verdict = NameVerdict::Review;
        }
        Ok(verdict)
    }

    /// Step 6. PII scan; redact spans with `confidence >= cfg.pii_redact_conf` in place with the
    /// literal `‹redacted:‹{pii_type}››`, set `pii_review = true` for any lower-confidence span
    /// (SA-PII-01). String values, numbers and object keys are all read (SA-PII-02). Returns
    /// `(redacted_content, pii_review)`.
    // spec: phase-1-analysis#SA-PII-02
    // One scan of one view, so a detector sees a single document and a pointer into a value is the
    // same pointer into `content`. A number holding a span becomes the string that replaces it.
    pub async fn pii_scan(&self, content: &Json) -> Result<(Json, bool), AppError> {
        let view = ScreeningView::of(content);
        let spans = self.pii.scan(&view.json).await?;
        let mut redacted = content.clone();
        let mut renamed: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let mut pii_review = false;
        let mut redacted_count = 0u32;
        let mut flagged_count = 0u32;

        // Apply redactions per located string, highest-offset-first so earlier offsets stay valid as
        // bytes are spliced out and the replacement token inserted.
        let mut by_pointer: std::collections::BTreeMap<String, Vec<PiiSpan>> =
            std::collections::BTreeMap::new();
        for span in spans {
            if span.confidence >= self.cfg.pii_redact_conf {
                by_pointer
                    .entry(span.json_pointer.clone())
                    .or_default()
                    .push(span);
            } else {
                pii_review = true;
                flagged_count += 1;
            }
        }
        for (pointer, mut spans) in by_pointer {
            spans.sort_by_key(|s| std::cmp::Reverse(s.start));
            if let Some(target) = pointer_get_str(&view.json, &pointer) {
                let mut s = target.to_string();
                for span in &spans {
                    let token = format!("‹redacted:‹{}››", span.pii_type);
                    let (start, end) = (span.start as usize, span.end as usize);
                    if start <= end
                        && end <= s.len()
                        && s.is_char_boundary(start)
                        && s.is_char_boundary(end)
                    {
                        s.replace_range(start..end, &token);
                        redacted_count += 1;
                    } else {
                        // An out-of-range/non-boundary span cannot be safely redacted in place; flag for
                        // review rather than corrupt the string.
                        pii_review = true;
                        flagged_count += 1;
                    }
                }
                match view.key_at(&pointer) {
                    Some(key) => {
                        renamed.insert(key.to_string(), s);
                    }
                    None => pointer_set_str(&mut redacted, &pointer, s),
                }
            } else {
                // The pointer did not resolve to a string; flag the uncertainty for review.
                pii_review = true;
                flagged_count += 1;
            }
        }
        // After the values, whose pointers name the keys as they arrived.
        if !renamed.is_empty() {
            rename_keys(&mut redacted, &renamed);
        }

        tracing::info!(
            target: "memory",
            redacted_count,
            flagged_count,
            "write.pii.scanned"
        );
        Ok((redacted, pii_review))
    }

    /// Step 7a. Detect imperative/instruction-like content over the content's string values
    /// (case-insensitive), and known phrases in its object keys (SA-PII-02). `is_instruction_like` is
    /// `true` when `matched_patterns >= 1` (ADR-008).
    pub fn detect_instruction(&self, content: &Json) -> InstructionLikelihood {
        instruction_likelihood(content)
    }

    /// Step 7b. The write gate. `trust = clamp01(0.6 * confidence + 0.4 * source_trust)`; when
    /// `instr.is_instruction_like`, trust is capped strictly below `trust_quarantine` so it can never
    /// admit or quarantine. Then maps to a three-way decision (SA-WGATE-01).
    pub fn write_gate(
        &self,
        confidence: f64,
        source_trust: f64,
        instr: InstructionLikelihood,
    ) -> GateDecision {
        let trust = self.trust_score(confidence, source_trust, instr);
        if trust >= self.cfg.trust_admit {
            GateDecision::Admit
        } else if trust >= self.cfg.trust_quarantine {
            GateDecision::Quarantine
        } else {
            GateDecision::Reject
        }
    }

    /// The trust score the gate compares against the bands; isolated so the persisted/quarantined
    /// trust value matches the decision exactly. Crate-visible so the Service's team import (ordin
    /// change M-7 b) records on a quarantined fact the score its gate decision used.
    pub(crate) fn trust_score(
        &self,
        confidence: f64,
        source_trust: f64,
        instr: InstructionLikelihood,
    ) -> f64 {
        let mut trust = clamp01(0.6 * confidence + 0.4 * source_trust.clamp(0.0, 1.0));
        if instr.is_instruction_like {
            trust = trust.min(self.cfg.trust_quarantine - 0.0001);
        }
        trust
    }

    /// Step 8. Embed fact content and persist `Fact` + vector via `MemoryStore::put_fact` then
    /// `set_fact_embedding`. Asserts the embedding length `== cfg.embed_dim` (SA-EMBED-01); a mismatch
    /// is a terminal `VAL_OUT_OF_RANGE`.
    pub async fn embed_and_persist(&self, ctx: &ScopeContext, fact: &Fact) -> Result<(), AppError> {
        let text = crate::store::fact_text(fact);
        let vectors = self.embed.embed(&[text]).await?;
        let vector = vectors.into_iter().next().ok_or_else(|| {
            AppError::Validation(
                ValidationKind::OutOfRange,
                "embedding provider returned no vector".into(),
            )
        })?;
        if vector.len() as u32 != self.cfg.embed_dim {
            return Err(AppError::Validation(
                ValidationKind::OutOfRange,
                format!(
                    "embedding length {} != MEMORY_EMBED_DIM {}",
                    vector.len(),
                    self.cfg.embed_dim
                ),
            ));
        }
        // Persist the fact (idempotent on the derived id) then attach the embedding + model version.
        self.storage.put_fact(fact).await?;
        self.storage
            .set_fact_embedding(ctx, &fact.id, &vector, &self.cfg.embed_model_version)
            .await?;
        Ok(())
    }

    /// Persist a gate-quarantined record into the `quarantine` table (never the `fact` table) through
    /// the storage port's `QuarantineStore`, which upserts on the idempotency key so a queue replay
    /// does not duplicate the row. The namespace is the record's `owner.tenant`, which the caller
    /// builds from the same job scope as `_ctx`.
    pub async fn quarantine(
        &self,
        _ctx: &ScopeContext,
        qr: &QuarantineRecord,
    ) -> Result<(), AppError> {
        self.storage.put_quarantine(qr).await?;
        Ok(())
    }

    // ---- helpers ----

    /// Upsert the provenance `Source` when one was supplied, returning its id. A new source is assigned
    /// `MEMORY_SOURCE_TRUST_DEFAULT`; the source id is derived deterministically from
    /// `(idempotency_key, origin_ref)` is not required here — provenance upsert is idempotent by record
    /// id, and a fresh id is fine because a replayed job reuses the same derived fact id regardless.
    async fn upsert_source(
        &self,
        ctx: &ScopeContext,
        source: Option<&SourceInput>,
        collection: &str,
    ) -> Result<Option<String>, AppError> {
        let Some(si) = source else {
            return Ok(None);
        };
        let owner = ScopeRef {
            tenant: ctx.tenant.clone(),
            team: ctx.teams.first().cloned(),
            user: ctx.user.clone(),
        };
        let id = source_id(&owner, &si.origin_ref);
        // Reuse an existing source's trust signal when the same source id is already present.
        let existing = self.storage.get_source(ctx, &id).await?;
        let trust = existing
            .as_ref()
            .map(|s| s.trust_signal)
            .unwrap_or(self.cfg.source_trust_default);
        let src = Source {
            id: id.clone(),
            origin_ref: si.origin_ref.clone(),
            modification_marker: si.modification_marker.clone(),
            trust_signal: trust,
            owner,
            // A new source takes the write's collection; an existing one keeps its own (ADR-020).
            collection: existing
                .as_ref()
                .map_or_else(|| collection.to_string(), |s| s.collection.clone()),
        };
        self.storage.put_source(&src).await?;
        Ok(Some(id))
    }

    /// Read the source trust used by scoring + the gate. A source-less / agent-stated fact uses
    /// `source_trust = 1.0`; a known source uses its `trust_signal`; a brand-new source uses the
    /// configured default.
    async fn read_source_trust(
        &self,
        ctx: &ScopeContext,
        source_id: Option<&str>,
    ) -> Result<f64, AppError> {
        match source_id {
            None => Ok(1.0),
            Some(id) => Ok(self
                .storage
                .get_source(ctx, id)
                .await?
                .map(|s| s.trust_signal)
                .unwrap_or(self.cfg.source_trust_default)),
        }
    }

    /// Derive the persisted fact id. When the job carries an `idempotency_key`, the id is a UUIDv5-style
    /// deterministic uuid over `(scope, idempotency_key, fact_index)`, the scope job dedupe uses, so a
    /// queue replay reuses the same id (idempotent persist via the C1 `UPSERT`) and two callers using
    /// one key never meet; otherwise a fresh UUIDv7.
    fn derive_fact_id(
        &self,
        scope: &ScopeRef,
        idempotency_key: Option<&str>,
        fact_index: usize,
    ) -> String {
        match idempotency_key {
            Some(key) => {
                let seed = owner_seed("fact", scope, &format!("{key}:{fact_index}"));
                format!("fact:{}", deterministic_uuid(&seed))
            }
            None => format!("fact:{}", Uuid::now_v7()),
        }
    }

    /// `WorkQueue::complete`, mapping a queue error to `AppError::Queue`. The tenant comes from the
    /// job's own scope, so completion cannot land in a namespace other than the one it was claimed
    /// from (RISK-016).
    async fn complete(&self, tenant: &str, job_id: &str) -> Result<(), AppError> {
        self.storage
            .complete(tenant, job_id)
            .await
            .map_err(AppError::Queue)
    }
}

/// What steps 6′ and 7′ found in the stated names and kinds.
#[derive(Default)]
struct NamesReview {
    instruction_like: bool,
    review: bool,
}

/// Step 2 (addition): each entity assertion becomes a mention after the subject/object mentions; a
/// name normalising to an existing mention's surface is merged with it.
fn add_asserted_mentions(mentions: &mut Vec<EntityMention>, asserted: &[EntityAssertion]) {
    for a in asserted {
        let n = normalise_name(&a.name);
        if n.is_empty() || mentions.iter().any(|m| normalise_name(&m.surface) == n) {
            continue;
        }
        mentions.push(EntityMention {
            surface: a.name.clone(),
            canonical_name: None,
        });
    }
}

// --- free helpers (pure) ---------------------------------------------------------------------------

/// Order the four outcomes so the aggregate report reflects the strongest per-job result.
fn stronger(a: WriteOutcome, b: WriteOutcome) -> WriteOutcome {
    fn rank(o: WriteOutcome) -> u8 {
        match o {
            WriteOutcome::Persisted => 3,
            WriteOutcome::Quarantined => 2,
            WriteOutcome::Rejected => 1,
            WriteOutcome::FilteredNoise | WriteOutcome::Erased => 0,
        }
    }
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

/// Whether an `AppError` should be retried (transient) vs dead-lettered immediately (terminal),
/// per the C4 Error Table.
fn is_retryable(err: &AppError) -> bool {
    match err {
        AppError::Store(StoreError::Validation(_) | StoreError::ScopeForbidden) => false,
        AppError::Store(_) => true,
        AppError::Queue(_) => true,
        AppError::Provider(ProviderError::Timeout)
        | AppError::Provider(ProviderError::Status(_)) => true,
        AppError::Provider(_) => false,
        AppError::Validation(_, _) => false,
        _ => false,
    }
}

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

/// Append `id` to `ids` unless already present (entity-id dedup within one fact).
fn push_unique(ids: &mut Vec<String>, id: String) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

/// Total length of non-whitespace characters across every string leaf in the JSON value.
fn non_ws_string_len(v: &Json) -> usize {
    let mut total = 0usize;
    for_each_string(v, &mut |s| {
        total += s.chars().filter(|c| !c.is_whitespace()).count();
    });
    total
}

/// Visit every object key in a JSON value, at every depth.
fn for_each_key(v: &Json, f: &mut impl FnMut(&str)) {
    match v {
        Json::Array(a) => a.iter().for_each(|e| for_each_key(e, f)),
        Json::Object(m) => m.iter().for_each(|(k, e)| {
            f(k);
            for_each_key(e, f);
        }),
        _ => {}
    }
}

/// `v` with every number written as a string, so a detector reads it and a pointer still finds it.
fn numbers_as_strings(v: &Json) -> Json {
    match v {
        Json::Number(n) => Json::String(n.to_string()),
        Json::Array(a) => Json::Array(a.iter().map(numbers_as_strings).collect()),
        Json::Object(m) => Json::Object(
            m.iter()
                .map(|(k, e)| (k.clone(), numbers_as_strings(e)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// What step 6 hands the detector: the content, numbers written as strings, plus a list of its object
/// keys under a key the content does not use.
struct ScreeningView {
    json: Json,
    /// The pointer prefix of the key list, `/<slot>/`.
    keys_at: Option<String>,
    keys: Vec<String>,
}

impl ScreeningView {
    fn of(content: &Json) -> Self {
        let mut json = numbers_as_strings(content);
        let mut keys = std::collections::BTreeSet::new();
        for_each_key(content, &mut |k| {
            keys.insert(k.to_string());
        });
        let keys: Vec<String> = keys.into_iter().collect();
        let mut keys_at = None;
        if let (Json::Object(m), false) = (&mut json, keys.is_empty()) {
            let mut slot = String::from("‹keys›");
            while m.contains_key(&slot) {
                slot.push('›');
            }
            m.insert(slot.clone(), Json::from(keys.clone()));
            keys_at = Some(format!("/{slot}/"));
        }
        Self {
            json,
            keys_at,
            keys,
        }
    }

    /// The object key a span's pointer locates, when it points into the key list.
    fn key_at(&self, pointer: &str) -> Option<&str> {
        let index = pointer.strip_prefix(self.keys_at.as_deref()?)?;
        self.keys
            .get(index.parse::<usize>().ok()?)
            .map(String::as_str)
    }
}

/// Rename the keys of `renamed` at every depth. Two keys of one object that redact to the same text
/// stay apart: dropping one would lose a value.
fn rename_keys(v: &mut Json, renamed: &std::collections::BTreeMap<String, String>) {
    match v {
        Json::Array(a) => a.iter_mut().for_each(|e| rename_keys(e, renamed)),
        Json::Object(m) => {
            let mut out = serde_json::Map::new();
            for (key, mut value) in std::mem::take(m) {
                rename_keys(&mut value, renamed);
                let name = renamed.get(&key).cloned().unwrap_or(key);
                let mut free = name.clone();
                let mut n = 2;
                while out.contains_key(&free) {
                    free = format!("{name} #{n}");
                    n += 1;
                }
                out.insert(free, value);
            }
            *m = out;
        }
        _ => {}
    }
}

/// Visit every string leaf in a JSON value.
fn for_each_string(v: &Json, f: &mut impl FnMut(&str)) {
    match v {
        Json::String(s) => f(s),
        Json::Array(a) => a.iter().for_each(|e| for_each_string(e, f)),
        Json::Object(m) => m.values().for_each(|e| for_each_string(e, f)),
        _ => {}
    }
}

/// Collapse internal whitespace runs to a single space and trim, after Unicode NFC normalisation.
fn collapse_ws(s: &str) -> String {
    let nfc = nfc(s);
    nfc.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Minimal Unicode NFC normalisation. The crate avoids a heavy dependency; the canonicalisation that
/// matters for recall is whitespace + key order, so NFC here is the identity over already-composed
/// input. (Documented assumption: input is expected NFC; full decomposition/recomposition is deferred.)
fn nfc(s: &str) -> String {
    s.to_string()
}

/// Recursively normalise a JSON value: object keys sorted (BTreeMap ordering), string values
/// whitespace-collapsed + NFC.
fn normalise_json(v: &Json) -> Json {
    match v {
        Json::String(s) => Json::String(collapse_ws(s)),
        Json::Array(a) => Json::Array(a.iter().map(normalise_json).collect()),
        Json::Object(m) => {
            let mut sorted = serde_json::Map::new();
            for (k, val) in m.iter() {
                sorted.insert(k.clone(), normalise_json(val));
            }
            // serde_json::Map preserves insertion order unless the `preserve_order` feature is off, in
            // which case it is a BTreeMap (already sorted). Insert in sorted key order to be
            // deterministic in both builds.
            let mut keys: Vec<&String> = sorted.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for k in keys {
                out.insert(k.clone(), sorted.get(k).cloned().unwrap_or(Json::Null));
            }
            Json::Object(out)
        }
        other => other.clone(),
    }
}

/// Normalise a surface form for the rules tier: `memory::text::normalise_name`, which this step's
/// normaliser moved to (SA-GRAPH-01, behaviour unchanged).
fn normalise_surface(s: &str) -> String {
    normalise_name(s)
}

/// The best ML-tier candidate at or above the similarity admit threshold, if exactly one stands out.
/// Similarity is a normalised character-bigram Dice coefficient between the normalised surface form and
/// each candidate's canonical name.
fn best_similarity_match(normalised: &str, candidates: &[Entity]) -> Option<String> {
    let mut best: Option<(String, f64)> = None;
    for c in candidates {
        let sim = dice_bigram(normalised, &normalise_surface(&c.canonical_name));
        if sim >= ML_SIMILARITY_ADMIT {
            match &best {
                Some((_, b)) if *b >= sim => {}
                _ => best = Some((c.id.clone(), sim)),
            }
        }
    }
    best.map(|(id, _)| id)
}

/// Sørensen–Dice coefficient over character bigrams (`memory::text`).
fn dice_bigram(a: &str, b: &str) -> f64 {
    crate::text::dice_bigram(a, b)
}

/// Whether the content names a predicate/relation (used in the salience heuristic).
fn has_predicate(content: &Json) -> bool {
    if let Json::Object(m) = content {
        m.contains_key("predicate") || m.contains_key("relation") || m.contains_key("verb")
    } else {
        false
    }
}

/// Deterministically derive entity mentions from agent-stated content: the string values of the
/// well-known `subject`/`object` fields, then any other string leaf, as raw mentions.
fn scan_entity_mentions(content: &Json) -> Vec<EntityMention> {
    let mut out = Vec::new();
    if let Json::Object(m) = content {
        for key in ["subject", "object"] {
            if let Some(Json::String(s)) = m.get(key) {
                if !s.trim().is_empty() {
                    out.push(EntityMention {
                        surface: s.clone(),
                        canonical_name: None,
                    });
                }
            }
        }
    }
    out
}

/// Synthesise a single entity name from content when the extractor offered no usable mention: prefer a
/// `subject` string, else the first string leaf, normalised. A value step 6 redacted names nothing, so
/// it is passed over while another leaf remains. Cut to `NAME_MAX_CHARS`: redaction can lengthen a text
/// that passed intake, and the fact keeps the whole of it.
// spec: phase-1-analysis#SA-WRITE-STRUCTURED-02
fn synth_entity_name(content: &Json) -> String {
    first_text(content).chars().take(NAME_MAX_CHARS).collect()
}

/// Whether step 4 could not name this write: `content.subject` or `content.object` over
/// `NAME_MAX_CHARS` once normalised, or, with no name to use (`names_entity`: the request states an
/// entity), its first text. The Service refuses such a write before it is queued, since it would fail
/// for good with no fact id to delete by.
// spec: phase-1-analysis#SA-WRITE-STRUCTURED-02
pub(crate) fn first_text_too_long(content: &Json, names_entity: bool) -> bool {
    let content = normalise_json(content);
    let over = |name: &str| name.chars().count() > NAME_MAX_CHARS;
    let named: Vec<String> = scan_entity_mentions(&content)
        .iter()
        .map(|m| normalise_surface(&m.surface))
        .filter(|n| !n.is_empty())
        .collect();
    named.iter().any(|n| over(n))
        || (named.is_empty() && !names_entity && over(&first_text(&content)))
}

/// The text a write with no named entity is anchored on.
fn first_text(content: &Json) -> String {
    if let Json::Object(m) = content {
        if let Some(Json::String(s)) = m.get("subject") {
            let n = normalise_surface(s);
            if !n.is_empty() && !s.contains(REDACTED_MARK) {
                return n;
            }
        }
    }
    let (mut first, mut redacted) = (String::new(), String::new());
    for_each_string(content, &mut |s| {
        let n = normalise_surface(s);
        if s.contains(REDACTED_MARK) {
            if redacted.is_empty() {
                redacted = n;
            }
        } else if first.is_empty() {
            first = n;
        }
    });
    if first.is_empty() {
        redacted
    } else {
        first
    }
}

/// SA-SCORE-01: a stated confidence scaled by its source's trust.
pub(crate) fn scaled_confidence(confidence: f64, source_trust: f64) -> f64 {
    clamp01(confidence * (0.5 + 0.5 * source_trust.clamp(0.0, 1.0)))
}

/// What `WritePipeline::screen_imported` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameVerdict {
    Clean,
    /// A low-confidence span: the facts naming it are held for a check.
    Review,
    Refused,
}

/// How many known instruction phrases an object key holds, read as words: `_`, `-`, `.` and a change
/// of case separate them.
fn count_key_phrases(key: &str) -> u32 {
    let mut words = String::with_capacity(key.len() + 4);
    let mut lower_before = false;
    for c in key.chars() {
        if matches!(c, '_' | '-' | '.') {
            words.push(' ');
        } else {
            if c.is_uppercase() && lower_before {
                words.push(' ');
            }
            words.push(c);
        }
        lower_before = c.is_lowercase();
    }
    count_name_phrases(&crate::text::normalise_kind(&words))
}

/// Known prompt-injection phrases, matched anywhere in a lower-cased string. Stated in step 7′ of the
/// spec: a change here changes that list.
const INSTRUCTION_PHRASES: &[&str] = &[
    "ignore previous",
    "ignore all previous",
    "disregard previous",
    "disregard the above",
    "system prompt",
    "you are now",
    "act as",
    "new instructions",
    "override",
    "jailbreak",
];

/// How many known instruction phrases a normalised name or kind holds: the whole check for one
/// (step 7′). `act as` must stand as two words and `override` is not looked for, because both occur
/// in ordinary names ("Contract assurance", "Manual override").
fn count_name_phrases(normalised: &str) -> u32 {
    let words: Vec<&str> = normalised
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    INSTRUCTION_PHRASES
        .iter()
        .filter(|p| match **p {
            "override" => false,
            "act as" => words.windows(2).any(|w| w == ["act", "as"]),
            phrase => normalised.contains(phrase),
        })
        .count() as u32
}

/// How many known instruction phrases a lower-cased string holds.
fn count_instruction_phrases(lower: &str) -> u32 {
    INSTRUCTION_PHRASES
        .iter()
        .filter(|p| lower.contains(**p))
        .count() as u32
}

/// Count imperative-mood and prompt-injection markers in a string (case-insensitive). The detector
/// matches a small allowlist of leading imperative verbs and known injection phrases; it is the
/// memory-poisoning defence (ADR-008) and never executes the content.
fn count_instruction_patterns(s: &str) -> u32 {
    let lower = s.to_lowercase();
    let mut count = count_instruction_phrases(&lower);

    // Leading imperative verbs (the first token of the trimmed string).
    const IMPERATIVES: &[&str] = &[
        "ignore",
        "disregard",
        "delete",
        "forget",
        "execute",
        "run",
        "do",
        "send",
        "reveal",
        "print",
        "output",
        "respond",
        "reply",
        "tell",
        "stop",
        "drop",
        "remove",
        "set",
        "update",
        "grant",
        "give",
    ];
    if let Some(first) = lower.split_whitespace().next() {
        let token = first.trim_matches(|c: char| c.is_ascii_punctuation());
        if IMPERATIVES.contains(&token) {
            count += 1;
        }
    }
    count
}

/// Resolve an RFC 6901 JSON pointer to a string value, if it points at one.
fn pointer_get_str<'a>(v: &'a Json, pointer: &str) -> Option<&'a str> {
    v.pointer(pointer).and_then(|p| p.as_str())
}

/// Set the string at an RFC 6901 JSON pointer (the pointer must already resolve to a string leaf).
fn pointer_set_str(v: &mut Json, pointer: &str, value: String) {
    if let Some(slot) = v.pointer_mut(pointer) {
        *slot = Json::String(value);
    }
}

/// The id of `owner`'s row for an origin: nothing one account cites reaches another's.
// One origin cited by two owners has two rows, each with its trust: from `origin_ref` alone, the first
// citer would own the row every later citer read and rewrote.
pub fn source_id(owner: &ScopeRef, origin_ref: &str) -> String {
    let seed = owner_seed("source", owner, origin_ref);
    format!("source:{}", deterministic_uuid(&seed))
}

/// The seed of an id one owner derives: each part encoded, so no two parts run together.
fn owner_seed(kind: &str, owner: &ScopeRef, value: &str) -> String {
    serde_json::json!([kind, owner.tenant, owner.team, owner.user, value]).to_string()
}

/// A deterministic UUID (v5, DNS namespace) over a seed string — stable across replays for idempotent
/// id derivation.
fn deterministic_uuid(seed: &str) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_DNS, seed.as_bytes())
}

#[cfg(test)]
mod tests;
