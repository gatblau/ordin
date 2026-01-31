//! C9 — Service Layer: the transport-agnostic per-operation orchestration core of `recall` (ADR-016).
//!
//! This module owns the security-critical chain that previously lived inside the C8 axum handlers
//! (the `EdgePipeline`): authenticate (C3) → authorise the operation class → per-`(subject, op-class)`
//! rate limiting → write idempotency (writes only) → invoke the owning component (C6 recall, C2 async
//! write, C1 fact read / retire / hard delete) → append-only audit. It knows nothing of HTTP, MCP,
//! headers, JSON wire framing, or status codes: it consumes a verified-from-token [`ScopeContext`]
//! (never a request-supplied scope) and the transport-neutral request/response types, and returns a
//! typed [`CallResult`] or a typed [`AppError`]. Both edges (C8 HTTP, the MCP edge) are thin adapters
//! over it, so there is exactly one definition of auth/scope/rate/idempotency/audit (SA-SVC-01).
//!
//! Correlation-id minting stays at the edge — each transport assigns one and passes it in via
//! [`CallContext`]; everything downstream of it is transport-neutral.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::api::ratelimit::{OpClass, RateHeaders, TokenBucket, READ_BURST, WRITE_BURST};
use crate::auth::{CallerAuthenticator, Op};
use crate::config::Config;
use crate::error::{AppError, AuthKind, ValidationKind};
use crate::obs::metrics::Metrics;
use crate::retrieval::{RecallOutcome, RetrievalEngine};
use crate::types::api::{
    Capabilities, DeletionProof, EntityAssertion, ExportPage, ExportRequest, ImportReport,
    ImportRequest, ImportResult, JobAckStatus, LinkAssertion, ListFactsPage, ListFactsRequest,
    RecallRequest, RememberRequest, RetireAck, WriteAck,
};
pub use crate::types::api::{
    CorrectLinkRequest, EntityDetailRequest, FactConflictItem, FactConflictsRequest,
    FactConflictsResult, FactHistory, FactHistoryRequest, FactOriginsRequest, FactVersion,
    ListEntitiesRequest, OriginFact,
};
use crate::types::domain::{AliasFolder, Entity, Fact, MemoryClass, Relationship, Source};
use crate::types::job::{JobKind, JobStatus, WorkJob};
use crate::types::ports::{AuditEntry, IdempotencyRecord, StoragePort, StoreError};
pub use crate::types::ports::{
    EntityCount, EntityDetail, EntityErasureProof, EntityPage, LinkRole,
};
use crate::types::scope::{ClassAuthorityDecision, ScopeContext, ScopeRef};
use crate::write_pipeline::{
    scaled_confidence, GateDecision, NameVerdict, QuarantineRecord, WritePipeline,
};

/// The `client` of the in-process ordin caller — the only caller for which `RememberRequest.visibility`,
/// `import_facts` and `clear_pii_review` are honoured (ordin changes M-6 a, M-7 b and c).
pub(crate) const IN_PROCESS_CLIENT: &str = "ordin-in-process";
/// The aliases an entity can hold after a file adds to it, as one assertion may state.
const IMPORTED_ALIASES: usize = 32;

/// `check_content`'s text bound, in characters (ADR-020).
const CHECK_CONTENT_MAX_CHARS: usize = 8_000;

/// `Service::check_content` request (ADR-020; ordin change M-8 f): 1–8,000 characters.
#[derive(Deserialize)]
pub struct CheckContentRequest {
    pub text: String,
}

/// What `Service::check_content` reports about a text (ADR-020).
#[derive(Serialize, Debug)]
pub struct ContentCheck {
    pub personal_data: PersonalDataFinding,
    pub instruction_like: bool,
}

/// Whether a text would pass the write pipeline's personal-data scan unchanged (`None`), be held for
/// review (`Review`), or be redacted (`Redact`).
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum PersonalDataFinding {
    None,
    Review,
    Redact,
}

/// `list_facts` page size (ordin change M-6 b).
const LIST_PAGE: u32 = 100;

/// `export_scope` page size (ordin change M-7 a).
const EXPORT_PAGE: u32 = 200;

/// The in-process rate-limiter state: one token bucket per `(subject, operation-class)`. Bounded — the
/// only in-process state the Service holds (the idempotency record and the audit trail live in C1's
/// store). Shared with the edge via `Arc` so both observe the same buckets.
pub type RateLimiter = Arc<tokio::sync::Mutex<HashMap<(String, OpClass), TokenBucket>>>;

/// The transport-agnostic core. Holds the injected component handles and is cheap to clone (Arc-backed).
pub struct Service {
    config: Arc<Config>,
    #[allow(dead_code)]
    // metrics seam wired through for parity with the edge state; emitted later.
    metrics: Arc<Metrics>,
    /// The storage port: facts, audit, idempotency records and the work queue.
    storage: Arc<dyn StoragePort>,
    engine: Arc<RetrievalEngine>,
    /// The caller authenticator: the OIDC `Authenticator` at the standalone edges, or whatever the
    /// embedding host registers.
    auth: Arc<dyn CallerAuthenticator>,
    rate: RateLimiter,
    /// The write pipeline whose public step functions `import_facts` runs (ordin change M-7 b).
    /// `None` until attached with [`Service::with_write_pipeline`].
    pipeline: Option<Arc<WritePipeline>>,
}

/// Per-call inputs every edge supplies. `idempotency_key` is required for writes, ignored for reads.
pub struct CallContext<'a> {
    /// Raw token value (after "Bearer "); `""` when absent.
    pub bearer: &'a str,
    /// Correlation id, minted by the edge.
    pub correlation_id: &'a str,
    /// Write-route idempotency key (the `Idempotency-Key` header value at the HTTP edge).
    pub idempotency_key: Option<&'a str>,
}

/// A point-in-time snapshot of the caller's rate bucket after this call's decision. The HTTP edge
/// renders it as `RateLimit-*`; the MCP edge may surface it as metadata. Mirrors [`RateHeaders`].
pub struct RateSnapshot {
    /// Per-minute refill rate for the bucket (the `RateLimit-Limit` value).
    pub limit: u32,
    /// Tokens remaining after this request's decrement.
    pub remaining: u32,
    /// Seconds until the bucket next has a token (`RateLimit-Reset`; also `Retry-After` on a reject).
    pub reset_secs: u64,
}

impl From<RateHeaders> for RateSnapshot {
    fn from(h: RateHeaders) -> Self {
        RateSnapshot {
            limit: h.limit,
            remaining: h.remaining,
            reset_secs: h.reset_secs,
        }
    }
}

/// Transport-neutral result: the typed payload plus the cross-cutting signals each edge renders.
pub struct CallResult<T> {
    /// The operation's typed payload.
    pub data: T,
    /// Rate snapshot for the caller's bucket after this call.
    pub rate: RateSnapshot,
    /// `true` on an idempotent replay (writes only).
    pub replayed: bool,
}

/// The error a call returns. Carries the typed [`AppError`] (the single classification the edge renders
/// to its transport representation) plus an optional [`RateSnapshot`].
///
/// The snapshot is `Some` for every failure that occurred **after** rate limiting passed or **on** rate
/// exhaustion (so the edge attaches `RateLimit-*`, and `Retry-After` on a 429), and `None` for an
/// authentication/authorisation failure that occurred before the bucket was touched (those carry no
/// rate signals, matching the C8 behaviour). The C9 spec's Internal Logic step 3 mandates the snapshot
/// reach the edge even on the 429 path; this wrapper is the channel for that (the bare `AppError` in the
/// spec's simplified signature cannot carry it).
pub struct CallError {
    /// The typed classification the edge renders.
    pub error: AppError,
    /// The caller's rate snapshot, when one was produced for this call.
    pub rate: Option<RateSnapshot>,
}

impl From<AppError> for CallError {
    fn from(error: AppError) -> Self {
        CallError { error, rate: None }
    }
}

/// The authenticated, authorised, rate-limited pre-amble of a call: produced by `begin`, consumed by
/// the per-operation body and by the audit step. The transport-neutral analogue of the old
/// `EdgePipeline` (sans HTTP headers).
struct Authorised {
    ctx: ScopeContext,
    operation: &'static str,
    rate: RateHeaders,
    /// The `fact_class` this call attempted to assert, on `remember` only. Set once the body has
    /// been deserialised and before the authority check, so the refusal that follows is audited
    /// with the class attempted rather than with a bare error code.
    ///
    /// The **label only** — never any part of `content`.
    // spec: components/service-layer#SA-AUTHZ-04
    fact_class: Option<String>,
}

impl Service {
    /// Construct the Service over the component stack. Called by `AppState::service` and by
    /// `embedded::build`; the same handles back both edges.
    pub fn new(
        config: Arc<Config>,
        metrics: Arc<Metrics>,
        storage: Arc<dyn StoragePort>,
        engine: Arc<RetrievalEngine>,
        auth: Arc<dyn CallerAuthenticator>,
        rate: RateLimiter,
    ) -> Self {
        Self {
            config,
            metrics,
            storage,
            engine,
            auth,
            rate,
            pipeline: None,
        }
    }

    /// Attach the write pipeline `import_facts` runs its personal-data scan, write gate and persist
    /// step through, so a team fact passes the same gate a `remember` does (ordin change M-7 b).
    // `Service::new` keeps its signature. Without one attached, `import_facts` fails with `INTERNAL`
    // after the client check.
    pub fn with_write_pipeline(mut self, pipeline: Arc<WritePipeline>) -> Self {
        self.pipeline = Some(pipeline);
        self
    }

    // --- The six operations ----------------------------------------------------------------------

    /// `GET /v1` (HTTP) — capabilities. Read class. Built from compile-time constants + config.
    pub async fn capabilities(
        &self,
        cx: CallContext<'_>,
    ) -> Result<CallResult<Capabilities>, CallError> {
        let pipe = self
            .begin(&cx, "capabilities", Op::Read, OpClass::Read)
            .await?;

        let openapi_url = format!("http://{}/openapi.json", self.config.http_addr);
        let caps = Capabilities {
            service: "memory".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            operations: vec![
                "recall".into(),
                "remember".into(),
                "get_fact".into(),
                "retire".into(),
                "delete".into(),
                "capabilities".into(),
                // The four graph reads (ADR-021); the person actions have no route.
                "fact_history".into(),
                "fact_origins".into(),
                "list_entities".into(),
                "entity_detail".into(),
                // ADR-022: a read for any client.
                "fact_conflicts".into(),
            ],
            openapi: openapi_url,
        };
        self.finish_success(pipe, caps).await
    }

    /// `GET /openapi.json` (HTTP) — runs the read chain (authenticate → authorise → rate-limit →
    /// audit) under the logical operation "openapi" and returns an empty payload; the edge renders the
    /// hand-built OpenAPI document itself (it is not a transport-neutral payload). Read class.
    pub async fn openapi(&self, cx: CallContext<'_>) -> Result<CallResult<()>, CallError> {
        let pipe = self.begin(&cx, "openapi", Op::Read, OpClass::Read).await?;
        self.finish_success(pipe, ()).await
    }

    /// `POST /v1/recall` (HTTP) — read. Validates the request range/class, delegates to C6.
    ///
    /// The request body arrives as raw bytes (rather than a pre-parsed `RecallRequest`) so the fixed
    /// chain ordering is preserved exactly: authenticate → authorise → rate-limit run **before** body
    /// deserialisation, so a malformed body on an unauthenticated request is a 401 (not a 400) and a
    /// body parse failure on an authenticated request is audited as `VAL_INVALID_BODY` — identical to
    /// the former C8 handler, where `EdgePipeline::begin` ran ahead of `serde_json::from_slice`.
    pub async fn recall(
        &self,
        cx: CallContext<'_>,
        body: &[u8],
    ) -> Result<CallResult<RecallOutcome>, CallError> {
        let pipe = self.begin(&cx, "recall", Op::Read, OpClass::Read).await?;

        // Deserialise the body; a malformed body is VAL_INVALID_BODY (audited on the error path).
        let req: RecallRequest = match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(_) => {
                return self
                    .finish_error(
                        pipe,
                        AppError::Validation(ValidationKind::InvalidBody, "recall body".into()),
                    )
                    .await
            }
        };

        // result_cap range [1,50] (the range is enforced here; C6 enforces the hard max).
        if req.result_cap < 1 || req.result_cap > 50 {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::OutOfRange, "result_cap".into()),
                )
                .await;
        }
        match self.engine.recall(&pipe.ctx, &req).await {
            Ok(outcome) => self.finish_success(pipe, outcome).await,
            Err(e) => self.finish_error(pipe, e).await,
        }
    }

    /// `POST /v1/memories` (HTTP) — write (async). Enqueues an `ExtractFact` job on C2.
    ///
    /// The body arrives as raw bytes (see `recall`) so the fixed chain ordering is preserved: the
    /// idempotency-key validation and replay run **before** body deserialisation, exactly as the former
    /// C8 handler did (key → replay → `serde_json::from_slice` → enqueue).
    pub async fn remember(
        &self,
        cx: CallContext<'_>,
        body: &[u8],
    ) -> Result<CallResult<WriteAck>, CallError> {
        const ROUTE: &str = "POST /v1/memories";
        let mut pipe = self
            .begin(&cx, "remember", Op::Write, OpClass::Write)
            .await?;

        // Chain step 4 — idempotency key required for writes.
        let key = match Self::require_idempotency_key(&cx) {
            Ok(k) => k,
            Err(e) => return self.finish_error(pipe, e).await,
        };
        // Replay a stored outcome if present (no new side effect).
        match self
            .idempotency_replay::<WriteAck>(&pipe, ROUTE, &key)
            .await
        {
            Ok(Some(data)) => return self.finish_replay(pipe, data).await,
            Ok(None) => {}
            Err(e) => return self.finish_error(pipe, e).await,
        }

        // Deserialise the request body (VAL_INVALID_BODY on failure).
        let req: RememberRequest = match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(_) => {
                return self
                    .finish_error(
                        pipe,
                        AppError::Validation(ValidationKind::InvalidBody, "remember body".into()),
                    )
                    .await
            }
        };
        // Chain step 5 — `content` must be a structured assertion object: recall holds no LLM and runs
        // no extraction (ADR-015), so a non-object `content` is a client error, not something to wrap.
        if !req.content.is_object() {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(
                        ValidationKind::InvalidBody,
                        "remember content must be a JSON object".into(),
                    ),
                )
                .await;
        }
        // `fact_class` is a caller-owned opaque label; recall bounds its length and nothing else.
        if let Some(fc) = req.fact_class.as_deref() {
            if fc.is_empty() || fc.chars().count() > 128 {
                return self
                    .finish_error(
                        pipe,
                        AppError::Validation(
                            ValidationKind::InvalidBody,
                            "remember fact_class must be 1..=128 characters".into(),
                        ),
                    )
                    .await;
            }
        }

        // A visibility is honoured for the in-process ordin caller alone (ordin change M-6 a); from any
        // other caller it is a malformed body, refused before anything is enqueued.
        if req.visibility.is_some() && !is_in_process(&pipe.ctx) {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::InvalidBody, "remember visibility".into()),
                )
                .await;
        }

        // Both name what only the host can vouch for, so anyone else setting them is a malformed body.
        // spec: phase-1-analysis#SA-WGATE-02
        if (req.gate_trust.is_some() || req.author.is_some()) && !is_in_process(&pipe.ctx) {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::InvalidBody, "remember host field".into()),
                )
                .await;
        }
        if req.gate_trust.is_some_and(|t| !(0.0..=1.0).contains(&t))
            || req
                .author
                .as_ref()
                .is_some_and(|a| a.is_empty() || a.len() > 128)
        {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::InvalidBody, "remember host field".into()),
                )
                .await;
        }

        // A collection breaking SA-COLL-01 is a malformed body, refused before anything is enqueued
        // (ADR-020).
        // spec: components/memory-store#SA-COLL-01
        if req
            .collection
            .as_deref()
            .is_some_and(|c| !crate::types::domain::is_valid_collection(c))
        {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::InvalidBody, "remember collection".into()),
                )
                .await;
        }

        // Step 5 (ADR-022), before the ADR-021 checks — one triple unless the caller classes the write a
        // passage, the writer's flags, confidence and start.
        // spec: components/service-layer#SA-TRIPLE-01
        if let Err(e) = check_triple_write(&req, &self.config.passage_class, Utc::now()) {
            return self.finish_error(pipe, e).await;
        }

        // Step 5 (ADR-021) — the entities, links and summary origins the write states.
        if let Err(e) = self.validate_graph_fields(&pipe.ctx, &req).await {
            return self.finish_error(pipe, e).await;
        }

        // A name the store cannot hold would fail the job for good, with no fact id to delete it by.
        // spec: phase-1-analysis#SA-WRITE-STRUCTURED-02
        if crate::write_pipeline::first_text_too_long(&req.content, !req.entities.is_empty()) {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::OutOfRange, "remember first text".into()),
                )
                .await;
        }

        // Chain step 5a — fact-class write authority. The single enforcement point, so both edges
        // inherit it; a check that lives on one edge is a check the other edge does not have.
        //
        // It sits HERE, and not inside `begin` as a fourth `Op`, for one reason that shows up only
        // in the audit trail: `begin` returns before an `Authorised` exists, so a refusal there
        // writes no audit record, and the requirement is that the refusal IS audited. On this path
        // `finish_error` audits. It is also the first point at which `fact_class` exists, the body
        // having just been parsed.
        //
        // It runs after the idempotency replay above: a replay is not a new assertion and must not
        // be re-judged, or a token whose authority narrowed after a successful write would start
        // failing a call that has no side effect left to prevent.
        //
        // Two consequences are deliberate rather than emergent. A refused write CONSUMES rate-limit
        // budget, because step 3 already decremented the bucket — probing which classes a token may
        // assert should cost the prober something. It consumes NO idempotency key, because the
        // outcome is persisted only after the component call succeeds, so the same key works once
        // the token is reissued correctly.
        // spec: phase-1-analysis#SA-AUTHZ-01
        pipe.fact_class = req.fact_class.clone();
        let decision = pipe.ctx.may_assert_class(
            req.fact_class.as_deref(),
            req.source.as_ref().map(|s| s.origin_ref.as_str()),
        );
        match decision {
            ClassAuthorityDecision::Admitted => {}
            ClassAuthorityDecision::AdmittedNoClaim => {
                // Admitted on the compatibility default. Counted so a deployment relying on the
                // control can tell it is not in force, instead of discovering that the way this
                // gap was discovered — from a 202 somebody expected to be a 403.
                // spec: phase-1-analysis#SA-AUTHZ-02
                self.metrics.inc_write_authority_absent();
            }
            refused => {
                if let Some(reason) = refused.refusal_reason() {
                    self.metrics.inc_write_authority_refusal(reason);
                }
                // The class label reaches the audit row (via `pipe.fact_class`) and the operator
                // log, and stops there. The response carries no class, no tenant and no record id,
                // so it discloses nothing about what exists.
                // spec: components/service-layer#SA-AUTHZ-06
                tracing::warn!(
                    target: "memory::service",
                    event = "write_authority_refused",
                    reason = refused.refusal_reason().unwrap_or("class"),
                    subject = %pipe.ctx.user,
                    jti = %pipe.ctx.token_jti,
                    client = pipe.ctx.client.as_deref().unwrap_or("-"),
                    correlation_id = %pipe.ctx.correlation_id,
                    "write refused: the token may not assert this class of fact"
                );
                return self.finish_error(pipe, AppError::ClassForbidden).await;
            }
        }

        // The queue's limit: a write past it waits for the workers instead of growing the queue. It
        // reads as a rate limit, which is what a caller should do about it.
        // spec: phase-1-analysis#SA-QUEUE-04
        let team = ScopeRef::from(&pipe.ctx).team;
        match self
            .storage
            .open_jobs(&pipe.ctx.tenant, team.as_deref())
            .await
        {
            Ok(open) if open >= u64::from(self.config.queue_max_open) => {
                return self.finish_error(pipe, AppError::RateLimited).await
            }
            Ok(_) => {}
            Err(e) => return self.finish_error(pipe, AppError::Queue(e)).await,
        }

        // Build and enqueue an ExtractFact WorkJob; the as-built handler sets every field.
        let mut payload = remember_payload(&req);
        if !req.derived_from.is_empty() {
            payload["origin_teams"] = serde_json::to_value(&pipe.ctx.teams).unwrap_or(Value::Null);
        }
        let job = WorkJob {
            id: format!("work_job:{}", Uuid::now_v7()),
            kind: JobKind::ExtractFact,
            payload,
            scope: ScopeRef::from(&pipe.ctx),
            idempotency_key: Some(key.clone()),
            attempts: 0,
            status: JobStatus::Pending,
            not_before: Utc::now(),
            created_at: Utc::now(),
            leased_until: None,
        };
        let job_id = match self.storage.enqueue(job).await {
            Ok(id) => id,
            Err(e) => return self.finish_error(pipe, AppError::Queue(e)).await,
        };

        let ack = WriteAck {
            job_id: job_id.clone(),
            status: JobAckStatus::Accepted,
        };
        // Persist the outcome for an idempotent replay. The stored body carries `already-accepted`, so
        // a replay deserialises back to `AlreadyAccepted` (carried over from the C8 handler).
        let stored = remember_stored_body(&pipe.ctx.correlation_id, &job_id);
        if let Err(e) = self
            .idempotency_persist(&pipe, ROUTE, &key, 202, &stored)
            .await
        {
            return self.finish_error(pipe, e).await;
        }
        self.finish_success(pipe, ack).await
    }

    /// `GET /v1/memories/{id}` (HTTP) — read. Returns the full `Fact`; conditional-GET (ETag /
    /// `If-Modified-Since`) is an HTTP optimisation the edge layers over this method.
    pub async fn get_fact(
        &self,
        cx: CallContext<'_>,
        id: &str,
    ) -> Result<CallResult<Fact>, CallError> {
        let pipe = self.begin(&cx, "get_fact", Op::Read, OpClass::Read).await?;

        match self.storage.get_fact(&pipe.ctx, id).await {
            Ok(Some(fact)) => self.finish_success(pipe, fact).await,
            Ok(None) => self.finish_error(pipe, AppError::NotFound).await,
            Err(e) => self.finish_error(pipe, AppError::Store(e)).await,
        }
    }

    /// `POST /v1/memories/{id}/retire` (HTTP) — forget (non-destructive). Calls C1 `end_validity`.
    pub async fn retire(
        &self,
        cx: CallContext<'_>,
        id: &str,
    ) -> Result<CallResult<RetireAck>, CallError> {
        const ROUTE: &str = "POST /v1/memories/{id}/retire";
        let pipe = self
            .begin(&cx, "retire", Op::Forget, OpClass::Write)
            .await?;

        let key = match Self::require_idempotency_key(&cx) {
            Ok(k) => k,
            Err(e) => return self.finish_error(pipe, e).await,
        };
        match self
            .idempotency_replay::<RetireAck>(&pipe, ROUTE, &key)
            .await
        {
            Ok(Some(data)) => return self.finish_replay(pipe, data).await,
            Ok(None) => {}
            Err(e) => return self.finish_error(pipe, e).await,
        }

        let now = Utc::now();
        match self.storage.end_validity(&pipe.ctx, id, now).await {
            Ok(()) => {}
            Err(StoreError::NotFound) => return self.finish_error(pipe, AppError::NotFound).await,
            Err(e) => return self.finish_error(pipe, AppError::Store(e)).await,
        }

        let ack = RetireAck {
            record_id: id.to_string(),
            retired_at: now,
        };
        let stored = serde_json::json!({
            "data": { "record_id": id, "retired_at": now.to_rfc3339() },
            "meta": { "correlation_id": pipe.ctx.correlation_id }
        });
        if let Err(e) = self
            .idempotency_persist(&pipe, ROUTE, &key, 200, &stored)
            .await
        {
            return self.finish_error(pipe, e).await;
        }
        self.finish_success(pipe, ack).await
    }

    /// `DELETE /v1/memories/{id}` (HTTP) — forget (verifiable hard delete). Calls C1 `hard_delete`
    /// directly (synchronous, proof-gated; SA-DELETE-01). The edge is not a `HardDelete` job producer.
    pub async fn delete(
        &self,
        cx: CallContext<'_>,
        id: &str,
    ) -> Result<CallResult<DeletionProof>, CallError> {
        const ROUTE: &str = "DELETE /v1/memories/{id}";
        let pipe = self
            .begin(&cx, "delete", Op::Forget, OpClass::Write)
            .await?;

        let key = match Self::require_idempotency_key(&cx) {
            Ok(k) => k,
            Err(e) => return self.finish_error(pipe, e).await,
        };
        match self
            .idempotency_replay::<DeletionProof>(&pipe, ROUTE, &key)
            .await
        {
            Ok(Some(data)) => return self.finish_replay(pipe, data).await,
            Ok(None) => {}
            Err(e) => return self.finish_error(pipe, e).await,
        }

        let proof = match self.storage.hard_delete(&pipe.ctx, id).await {
            Ok(p) => p,
            Err(StoreError::NotFound) => return self.finish_error(pipe, AppError::NotFound).await,
            Err(e) => return self.finish_error(pipe, AppError::Store(e)).await,
        };

        let stored = serde_json::json!({
            "data": serde_json::to_value(&proof).unwrap_or(Value::Null),
            "meta": { "correlation_id": pipe.ctx.correlation_id }
        });
        if let Err(e) = self
            .idempotency_persist(&pipe, ROUTE, &key, 200, &stored)
            .await
        {
            return self.finish_error(pipe, e).await;
        }
        self.finish_success(pipe, proof).await
    }

    // --- ordin changes M-6 b and M-7 (transport-neutral; no edge route) ----------------------------

    /// The caller's read-filtered facts, newest `ingested_at` first, keyset-paged 100 a page (ordin
    /// change M-6 b). Read class; any client.
    pub async fn list_facts(
        &self,
        cx: CallContext<'_>,
        req: ListFactsRequest,
    ) -> Result<CallResult<ListFactsPage>, CallError> {
        let pipe = self
            .begin(&cx, "list_facts", Op::Read, OpClass::Read)
            .await?;
        let after = match req.cursor.as_deref().map(ListCursor::decode).transpose() {
            Ok(c) => c.map(|c| (c.at, c.id)),
            Err(e) => return self.finish_error(pipe, e).await,
        };
        let mut facts = match self
            .storage
            .list_facts_page(&pipe.ctx, after, req.include_retired, LIST_PAGE + 1)
            .await
        {
            Ok(f) => f,
            Err(e) => return self.finish_error(pipe, AppError::Store(e)).await,
        };
        let next_cursor = if facts.len() > LIST_PAGE as usize {
            facts.truncate(LIST_PAGE as usize);
            let last = facts.last().ok_or(AppError::Internal);
            match last.and_then(|f| {
                ListCursor {
                    at: f.ingested_at,
                    id: f.id.clone(),
                }
                .encode()
            }) {
                Ok(c) => Some(c),
                Err(e) => return self.finish_error(pipe, e).await,
            }
        } else {
            None
        };
        self.finish_success(pipe, ListFactsPage { facts, next_cursor })
            .await
    }

    /// A page of 200 read-filtered facts of the caller's scope ordered by id, closed ones included,
    /// with every entity, source and relationship they name (ordin change M-7 a). Read class; any
    /// client. A relationship is named by a page when both its endpoints are entities the page's
    /// facts name.
    // A page's relationships are those linked through a `relationship_fact` row (either role, any
    // validity) to a fact on the page, read-filtered and deduplicated by id (service-layer amendment
    // January 2026; ordin SA-AMEM-04). `links_of_fact` is called per fact of the page.
    pub async fn export_scope(
        &self,
        cx: CallContext<'_>,
        req: ExportRequest,
    ) -> Result<CallResult<ExportPage>, CallError> {
        let pipe = self
            .begin(&cx, "export_scope", Op::Read, OpClass::Read)
            .await?;
        let after = match req.cursor.as_deref().map(ExportCursor::decode).transpose() {
            Ok(c) => c.map(|c| c.id),
            Err(e) => return self.finish_error(pipe, e).await,
        };
        match self.export_page(&pipe.ctx, after.as_deref()).await {
            Ok(page) => self.finish_success(pipe, page).await,
            Err(e) => self.finish_error(pipe, e).await,
        }
    }

    /// Structured facts with their own ids, through the personal-data scan and write gate of the
    /// write pipeline, without extraction (ordin change M-7 b). `ordin-in-process` only; one write
    /// token per call whatever the batch size.
    ///
    /// Per fact: a fact equal in every carried field to the stored fact of its id is `Unchanged` and
    /// nothing is written. Otherwise `pii_scan`, `detect_instruction` and `write_gate` decide, with
    /// the trust of the fact's source (the stored source's, else the batch's, else
    /// `source_trust_default`). A fact citing a source has its stated confidence scaled by that trust
    /// (SA-SCORE-01) before the gate, as any other write has; a fact citing none is scaled by nothing,
    /// as a source-less write is. Entity names, aliases and link kinds pass the phrase check and the
    /// personal-data scan: a hit refuses that item `Invalid`, and a low-confidence span holds the
    /// facts naming it for a check. `Admit` upserts the entities, source and relationships it names, then
    /// the fact through `embed_and_persist`, and appends an audit row of operation `import`;
    /// `Quarantine` writes a quarantine record keyed `"team:<fact id>"`; `Reject` writes nothing.
    // Both this and `clear_pii_review` are idempotent by content (a re-import is `Unchanged`; clearing
    // a cleared flag changes nothing). Neither requires nor stores an idempotency record; a key, if
    // given, is ignored.
    pub async fn import_facts(
        &self,
        cx: CallContext<'_>,
        req: ImportRequest,
    ) -> Result<CallResult<ImportReport>, CallError> {
        self.import(cx, req, ConfidenceIs::Stated).await
    }

    /// `import_facts` for facts this store scored already, a team file read back or a person's
    /// adoption of a read fact: their confidence went through its source's trust when it was first
    /// written, and is not scaled again.
    pub async fn import_scored_facts(
        &self,
        cx: CallContext<'_>,
        req: ImportRequest,
    ) -> Result<CallResult<ImportReport>, CallError> {
        self.import(cx, req, ConfidenceIs::Scored).await
    }

    async fn import(
        &self,
        cx: CallContext<'_>,
        req: ImportRequest,
        confidence: ConfidenceIs,
    ) -> Result<CallResult<ImportReport>, CallError> {
        let pipe = self
            .begin(&cx, "import_facts", Op::Write, OpClass::Write)
            .await?;
        if !is_in_process(&pipe.ctx) {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::InvalidBody, "import_facts".into()),
                )
                .await;
        }
        let Some(pipeline) = self.pipeline.clone() else {
            return self.finish_error(pipe, AppError::Internal).await;
        };
        match self.import_batch(&pipe, &pipeline, req, confidence).await {
            Ok(report) => self.finish_success(pipe, report).await,
            Err(e) => self.finish_error(pipe, e).await,
        }
    }

    /// Set `pii_review = false` on one fact the caller owns, leaving everything else as it is (ordin
    /// change M-7 c). `ordin-in-process` only.
    pub async fn clear_pii_review(
        &self,
        cx: CallContext<'_>,
        id: &str,
    ) -> Result<CallResult<()>, CallError> {
        let pipe = self
            .begin(&cx, "clear_pii_review", Op::Write, OpClass::Write)
            .await?;
        if !is_in_process(&pipe.ctx) {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::InvalidBody, "clear_pii_review".into()),
                )
                .await;
        }
        match self.storage.set_pii_review(&pipe.ctx, id, false).await {
            Ok(()) => self.finish_success(pipe, ()).await,
            Err(StoreError::NotFound) => self.finish_error(pipe, AppError::NotFound).await,
            Err(e) => self.finish_error(pipe, AppError::Store(e)).await,
        }
    }

    /// Report whether `text` would be redacted or held for a personal-data check, and whether it reads
    /// as an instruction — running the write pipeline's own `pii_scan` and `detect_instruction`, and
    /// writing nothing (ADR-020; ordin change M-8 f). `ordin-in-process` only (SA-SVC-CHECK-01); the
    /// text is never logged, audited or stored — only the outcome is audited.
    // spec: components/service-layer#SA-SVC-CHECK-01
    pub async fn check_content(
        &self,
        cx: CallContext<'_>,
        req: CheckContentRequest,
    ) -> Result<CallResult<ContentCheck>, CallError> {
        let pipe = self
            .begin(&cx, "check_content", Op::Read, OpClass::Read)
            .await?;
        if !is_in_process(&pipe.ctx) {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::InvalidBody, "check_content".into()),
                )
                .await;
        }
        let len = req.text.chars().count();
        if len == 0 || len > CHECK_CONTENT_MAX_CHARS {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::OutOfRange, "check_content text".into()),
                )
                .await;
        }
        // As for `import_facts`, the pipeline's steps are reached through the handle
        // `with_write_pipeline` attaches; without one the call fails with `INTERNAL`.
        // `Metrics` has no per-operation counter family; the outcome is in the audit row.
        let Some(pipeline) = self.pipeline.clone() else {
            return self.finish_error(pipe, AppError::Internal).await;
        };
        let content = serde_json::json!({ "statement": req.text });
        let (redacted, review) = match pipeline.pii_scan(&content).await {
            Ok(r) => r,
            Err(e) => return self.finish_error(pipe, e).await,
        };
        let personal_data = if redacted != content {
            PersonalDataFinding::Redact
        } else if review {
            PersonalDataFinding::Review
        } else {
            PersonalDataFinding::None
        };
        let instruction_like = pipeline.detect_instruction(&content).is_instruction_like;
        self.finish_success(
            pipe,
            ContentCheck {
                personal_data,
                instruction_like,
            },
        )
        .await
    }

    /// Read one export page: the facts after `after`, then the entities, sources and relationships
    /// they name, each read-filtered and listed once.
    async fn export_page(
        &self,
        ctx: &ScopeContext,
        after: Option<&str>,
    ) -> Result<ExportPage, AppError> {
        let mut facts = self
            .storage
            .export_facts_page(ctx, after, EXPORT_PAGE + 1)
            .await?;
        let next_cursor = if facts.len() > EXPORT_PAGE as usize {
            facts.truncate(EXPORT_PAGE as usize);
            let last = facts.last().ok_or(AppError::Internal)?;
            Some(
                ExportCursor {
                    id: last.id.clone(),
                }
                .encode()?,
            )
        } else {
            None
        };

        let entity_ids: BTreeSet<&String> = facts.iter().flat_map(|f| &f.entities).collect();
        let mut entities = Vec::with_capacity(entity_ids.len());
        for id in &entity_ids {
            // A private alias does not travel in an export.
            if let Some(e) = self.storage.get_entity(ctx, id).await? {
                entities.push(e.as_seen_by(false));
            }
        }
        let source_ids: BTreeSet<&String> =
            facts.iter().filter_map(|f| f.source_id.as_ref()).collect();
        let mut sources = Vec::with_capacity(source_ids.len());
        for id in source_ids {
            if let Some(s) = self.storage.get_source(ctx, id).await? {
                sources.push(s);
            }
        }
        let mut relationships: Vec<Relationship> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for f in &facts {
            for (r, _) in self.storage.links_of_fact(ctx, &f.id).await? {
                if seen.insert(r.id.clone()) {
                    relationships.push(r);
                }
            }
        }
        relationships.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(ExportPage {
            facts,
            entities,
            sources,
            relationships,
            next_cursor,
        })
    }

    /// The body of `import_facts`: one outcome per fact, then one `Invalid` per refused companion
    /// item. A store or provider failure fails the call; what was written before it stays, and a
    /// repeat returns `Unchanged` for it.
    async fn import_batch(
        &self,
        pipe: &Authorised,
        pipeline: &WritePipeline,
        req: ImportRequest,
        confidence: ConfidenceIs,
    ) -> Result<ImportReport, AppError> {
        let ctx = &pipe.ctx;
        let ImportRequest {
            facts,
            entities,
            sources,
            relationships,
        } = req;
        if facts.is_empty() && entities.is_empty() && sources.is_empty() && relationships.is_empty()
        {
            return Err(AppError::Validation(
                ValidationKind::InvalidBody,
                "batch".into(),
            ));
        }
        let mut refused: Vec<(String, ImportResult)> = Vec::new();

        // Companion items are written only under the caller's own scope; any other owner is refused
        // and never written (owners are the importing scope's, never the sharer's).
        // An id already held by anyone else is refused the same way, whatever the batch claims about
        // it, and with no word on whose it is.
        let mut batch_entities: HashMap<String, Entity> = HashMap::new();
        // The entities a low-confidence span in a name or kind holds for a check.
        let mut held: HashSet<String> = HashSet::new();
        for e in entities {
            if !owned_by_caller(ctx, &e.owner) {
                refused.push((e.id, invalid("owner")));
            } else if !is_record_id(&e.id, "entity:") || self.id_held_by_another(ctx, &e.id).await?
            {
                refused.push((e.id, invalid("id")));
            } else {
                let names: Vec<&str> = std::iter::once(e.canonical_name.as_str())
                    .chain(e.aliases.iter().map(String::as_str))
                    .collect();
                match pipeline.screen_imported(&names).await? {
                    NameVerdict::Refused => {
                        refused.push((e.id, invalid("entity names")));
                        continue;
                    }
                    NameVerdict::Review => {
                        held.insert(e.id.clone());
                    }
                    NameVerdict::Clean => {}
                }
                // No folder record travels in an export, and a caller's is not taken: an alias the
                // entity already has keeps its folders, and a new one belongs to the entity's own.
                let mut e = e;
                let stored = self.storage.get_entity(ctx, &e.id).await?;
                // A file another person can edit adds names to a stored entity and replaces none.
                if let (ConfidenceIs::Scored, Some(existing)) = (confidence, &stored) {
                    e.canonical_name = existing.canonical_name.clone();
                    e.collection = existing.collection.clone();
                    e.private_aliases = existing.private_aliases.clone();
                    let room = IMPORTED_ALIASES.saturating_sub(existing.aliases.len());
                    let added: Vec<String> = e
                        .aliases
                        .drain(..)
                        .filter(|alias| !existing.aliases.contains(alias))
                        .take(room)
                        .collect();
                    e.aliases = existing.aliases.iter().cloned().chain(added).collect();
                } else if confidence == ConfidenceIs::Scored {
                    e.aliases.truncate(IMPORTED_ALIASES);
                }
                e.alias_folders = imported_alias_folders(&e, stored.as_ref());
                batch_entities.insert(e.id.clone(), e);
            }
        }
        let mut batch_sources: HashMap<String, Source> = HashMap::new();
        for s in sources {
            if !owned_by_caller(ctx, &s.owner) {
                refused.push((s.id, invalid("owner")));
            } else if !is_record_id(&s.id, "source:") || self.id_held_by_another(ctx, &s.id).await?
            {
                refused.push((s.id, invalid("id")));
            } else {
                batch_sources.insert(s.id.clone(), s);
            }
        }
        let mut batch_relationships: Vec<Relationship> = Vec::new();
        for r in relationships {
            if !owned_by_caller(ctx, &r.owner) {
                refused.push((r.id, invalid("owner")));
            } else if !is_record_id(&r.id, "relationship:")
                || self.id_held_by_another(ctx, &r.id).await?
            {
                refused.push((r.id, invalid("id")));
            } else if confidence == ConfidenceIs::Scored
                && !self.cites_a_known_source(ctx, &r, &batch_sources).await?
            {
                refused.push((r.id, invalid("relationship source")));
            } else {
                match pipeline.screen_imported(&[r.kind.as_str()]).await? {
                    NameVerdict::Refused => {
                        refused.push((r.id, invalid("relationship kind")));
                        continue;
                    }
                    NameVerdict::Review => {
                        held.insert(r.from.clone());
                        held.insert(r.to.clone());
                    }
                    NameVerdict::Clean => {}
                }
                batch_relationships.push(r);
            }
        }

        // A relationship whose endpoints are not both entities of the batch or of the caller's scope
        // is not written (ordin SA-AMEM-04).
        let mut linkable: Vec<Relationship> = Vec::new();
        for r in batch_relationships {
            let mut ok = true;
            for e in [&r.from, &r.to] {
                if !batch_entities.contains_key(e)
                    && self.storage.get_entity(ctx, e).await?.is_none()
                {
                    ok = false;
                }
            }
            if ok {
                linkable.push(r);
            } else {
                refused.push((r.id, invalid("relationship endpoints")));
            }
        }

        let mut results = Vec::with_capacity(facts.len() + refused.len());
        let mut stored: Vec<Fact> = Vec::new();
        let mut admitted: Vec<String> = Vec::new();
        for fact in facts {
            let id = fact.id.clone();
            let kept = fact.clone();
            let outcome = self
                .import_one(
                    pipe,
                    pipeline,
                    fact,
                    &batch_entities,
                    &batch_sources,
                    &linkable,
                    &held,
                    confidence,
                )
                .await?;
            if matches!(outcome, ImportResult::Admitted | ImportResult::Unchanged) {
                stored.push(kept);
            }
            if outcome == ImportResult::Admitted {
                admitted.push(id.clone());
            }
            results.push((id, outcome));
        }
        // The batch's relationships after its entities, then each linked, role asserted, to every
        // stored fact of the batch naming both its ends (service-layer amendment; SA-AMEM-04). A fact
        // that arrived closed closes a link it alone asserts (the closing rule, 11′).
        //
        // From a file another person can edit, a link follows the fact asserting it: with none stored
        // it is not written. A link already stored is this computer's, as a stored source or entity
        // is: the file rewrites nothing on it and adds no asserter, and only a fact that already
        // asserts it can close it.
        // spec: phase-1-analysis#SA-GRAPH-15
        let mut closed: BTreeSet<String> = BTreeSet::new();
        for r in &linkable {
            let scored = confidence == ConfidenceIs::Scored;
            let known = scored
                && self
                    .storage
                    .record_owner(&ctx.tenant, &r.id)
                    .await?
                    .is_some();
            let asserting: Vec<&Fact> = stored
                .iter()
                .filter(|f| f.entities.contains(&r.from) && f.entities.contains(&r.to))
                .collect();
            if scored && !known && asserting.is_empty() {
                refused.push((r.id.clone(), invalid("relationship unasserted")));
                continue;
            }
            if !known {
                for e in [&r.from, &r.to] {
                    if let Some(ent) = batch_entities.get(e) {
                        self.storage.put_entity(ent).await?;
                    }
                }
                self.storage.put_relationship(r).await?;
            }
            for f in asserting {
                if known && !self.asserts(ctx, &f.id, &r.id).await? {
                    continue;
                }
                if !known {
                    self.storage
                        .link_fact(ctx, &r.id, &f.id, crate::types::ports::LinkRole::Asserted)
                        .await?;
                }
                if f.valid_to.is_some() {
                    closed.insert(f.id.clone());
                }
            }
        }
        for f in &stored {
            if let (true, Some(at)) = (closed.contains(&f.id), f.valid_to) {
                self.storage.end_validity(ctx, &f.id, at).await?;
            }
        }
        self.record_import_conflicts(ctx, &admitted).await?;
        results.extend(refused);
        Ok(ImportReport { results })
    }

    /// A link's source comes with the batch or is readable already, as a fact's must.
    async fn cites_a_known_source(
        &self,
        ctx: &ScopeContext,
        r: &Relationship,
        batch_sources: &HashMap<String, Source>,
    ) -> Result<bool, AppError> {
        let Some(id) = &r.source_id else {
            return Ok(true);
        };
        Ok(batch_sources.contains_key(id) || self.storage.get_source(ctx, id).await?.is_some())
    }

    /// Whether a stored fact already asserts a stored link.
    async fn asserts(&self, ctx: &ScopeContext, fact: &str, link: &str) -> Result<bool, AppError> {
        Ok(self
            .storage
            .links_of_fact(ctx, fact)
            .await?
            .iter()
            .any(|(r, role)| r.id == link && *role == crate::types::ports::LinkRole::Asserted))
    }

    /// What a held team fact names, in the form a person's keep writes it (SA-GRAPH-15): its things,
    /// and the links of the batch that would have followed it. Each name is the thing's own.
    async fn held_assertions(
        &self,
        ctx: &ScopeContext,
        fact: &Fact,
        batch_entities: &HashMap<String, Entity>,
        linkable: &[Relationship],
    ) -> Result<(Vec<EntityAssertion>, Vec<LinkAssertion>), AppError> {
        let mut known: HashMap<&str, Entity> = HashMap::new();
        for id in &fact.entities {
            let entity = match batch_entities.get(id) {
                Some(e) => Some(e.clone()),
                None => self.storage.get_entity(ctx, id).await?,
            };
            if let Some(e) = entity {
                known.insert(id.as_str(), e);
            }
        }
        let things = fact
            .entities
            .iter()
            .filter_map(|id| known.get(id.as_str()))
            .map(|e| EntityAssertion {
                name: e.canonical_name.clone(),
                aliases: e.aliases.iter().take(IMPORTED_ALIASES).cloned().collect(),
            })
            .collect();
        let links = linkable
            .iter()
            .filter_map(|r| {
                let (from, to) = (known.get(r.from.as_str())?, known.get(r.to.as_str())?);
                Some(LinkAssertion {
                    from: from.canonical_name.clone(),
                    kind: r.kind.clone(),
                    to: to.canonical_name.clone(),
                    ended: r.valid_to.is_some(),
                    confidence: Some(r.confidence),
                })
            })
            .collect();
        Ok((things, links))
    }

    /// Whether `id` already names a row that is not the caller's, readable or not.
    async fn id_held_by_another(&self, ctx: &ScopeContext, id: &str) -> Result<bool, AppError> {
        Ok(self
            .storage
            .record_owner(&ctx.tenant, id)
            .await?
            .is_some_and(|o| o.tenant != ctx.tenant || o.user != ctx.user))
    }

    /// Import one fact (see `import_facts`).
    #[allow(clippy::too_many_arguments)]
    async fn import_one(
        &self,
        pipe: &Authorised,
        pipeline: &WritePipeline,
        mut fact: Fact,
        batch_entities: &HashMap<String, Entity>,
        batch_sources: &HashMap<String, Source>,
        linkable: &[Relationship],
        held: &HashSet<String>,
        confidence: ConfidenceIs,
    ) -> Result<ImportResult, AppError> {
        let ctx = &pipe.ctx;
        if let Some(reason) = import_fact_defect(ctx, &fact) {
            return Ok(invalid(reason));
        }
        if self.id_held_by_another(ctx, &fact.id).await? {
            return Ok(invalid("id"));
        }
        // A stored fact opens the entities it names. As for a relationship's ends (ordin SA-AMEM-04):
        // each entity and the source must come with the batch or be readable already.
        for e in &fact.entities {
            if !batch_entities.contains_key(e) && self.storage.get_entity(ctx, e).await?.is_none() {
                return Ok(invalid("fact entities"));
            }
        }
        if let Some(sid) = &fact.source_id {
            if !batch_sources.contains_key(sid)
                && self.storage.get_source(ctx, sid).await?.is_none()
            {
                return Ok(invalid("fact source"));
            }
        }
        // SA-TRIPLE-12: a fact whose content parses as a triple is checked and gets its indexed parts,
        // its names resolved by exact normalised name; any other content is admitted as before.
        // spec: components/service-layer#SA-TRIPLE-12
        fact.triple_key = None;
        if fact.fact_class.as_deref() != Some(self.config.passage_class.as_str())
            && crate::triple::parses_as_triple(&fact.content)
        {
            let Ok(t) = crate::triple::check_triple(&fact.content) else {
                return Ok(invalid("content"));
            };
            match self
                .imported_triple_key(ctx, &t, &fact, batch_entities)
                .await?
            {
                Some(k) => fact.triple_key = Some(k),
                None => return Ok(invalid("fact entities")),
            }
        }

        // The scan writes nothing, so it runs before the comparison: a stored fact holds the redacted
        // content, and an unchanged fact must compare equal to it.
        let (content, pii_review) = pipeline.pii_scan(&fact.content).await?;
        let pii_review = pii_review || fact.entities.iter().any(|e| held.contains(e));

        let stored_source = match &fact.source_id {
            Some(sid) => self.storage.get_source(ctx, sid).await?,
            None => None,
        };
        let source_trust = match (&fact.source_id, &stored_source) {
            (_, Some(s)) => s.trust_signal,
            (Some(sid), None) => batch_sources
                .get(sid)
                .map(|s| s.trust_signal)
                .unwrap_or(self.config.source_trust_default),
            (None, None) => self.config.source_trust_default,
        };
        // A stored fact holds either what was stated, written here first, or its scaled form, as a
        // stored fact holds either the content or its redaction.
        let stated = fact.confidence;
        if confidence == ConfidenceIs::Stated && fact.source_id.is_some() {
            fact.confidence = scaled_confidence(stated, source_trust);
        }
        if let Some(stored) = self.storage.get_fact(ctx, &fact.id).await? {
            if carried_fields_equal(&stored, &fact, &content, stated) {
                return Ok(ImportResult::Unchanged);
            }
        }

        let instr = pipeline.detect_instruction(&content);
        let decision = pipeline.write_gate(fact.confidence, source_trust, instr);
        match decision {
            GateDecision::Reject => Ok(ImportResult::Rejected),
            GateDecision::Quarantine => {
                let (entities_asserted, links_asserted) = self
                    .held_assertions(ctx, &fact, batch_entities, linkable)
                    .await?;
                let qr = QuarantineRecord {
                    entities_asserted,
                    links_asserted,
                    derived_from: vec![],
                    id: format!("quarantine:{}", Uuid::now_v7()),
                    fact_id: Some(fact.id.clone()),
                    content,
                    entities: fact.entities.clone(),
                    source_id: fact.source_id.clone(),
                    memory_class: fact.memory_class,
                    fact_class: fact.fact_class.clone(),
                    owner: fact.owner.clone(),
                    trust_score: pipeline.trust_score(fact.confidence, source_trust, instr),
                    confidence: fact.confidence,
                    salience: fact.salience,
                    is_instruction_like: instr.is_instruction_like,
                    pii_review,
                    idempotency_key: Some(format!("team:{}", fact.id)),
                    reason: "trust in [trust_quarantine, trust_admit)".into(),
                    quarantined_at: Utc::now(),
                    collection: fact.collection.clone(),
                };
                pipeline.quarantine(ctx, &qr).await?;
                Ok(ImportResult::Quarantined)
            }
            GateDecision::Admit => {
                // Entities, the source and the relationships the fact names first, then the fact.
                for eid in &fact.entities {
                    if let Some(e) = batch_entities.get(eid) {
                        self.storage.put_entity(e).await?;
                    }
                }
                if let Some(sid) = &fact.source_id {
                    if let Some(s) = batch_sources.get(sid) {
                        // An existing source keeps its trust, and a file another person can edit
                        // does not rewrite the citation other facts show.
                        let mut s = s.clone();
                        if let Some(existing) = &stored_source {
                            s.trust_signal = existing.trust_signal;
                        }
                        if stored_source.is_none() || confidence == ConfidenceIs::Stated {
                            self.storage.put_source(&s).await?;
                        }
                    }
                }
                fact.content = content;
                fact.pii_review = pii_review;
                pipeline.embed_and_persist(ctx, &fact).await?;
                self.write_audit_as(pipe, "import", "success").await?;
                Ok(ImportResult::Admitted)
            }
        }
    }

    /// An imported triple's `TripleKey` (SA-TRIPLE-12): the subject and an entity object resolved by
    /// exact normalised name among the fact's own entities, then the batch's, then the store's; `None`
    /// when one resolves to no entity.
    async fn imported_triple_key(
        &self,
        ctx: &ScopeContext,
        t: &crate::types::api::Triple,
        fact: &Fact,
        batch_entities: &HashMap<String, Entity>,
    ) -> Result<Option<crate::types::domain::TripleKey>, AppError> {
        let mut candidates: Vec<Entity> = Vec::new();
        for id in &fact.entities {
            match batch_entities.get(id) {
                Some(e) => candidates.push(e.clone()),
                None => candidates.extend(self.storage.get_entity(ctx, id).await?),
            }
        }
        let mut by_name: HashMap<String, String> = HashMap::new();
        let mut names = vec![t.subject.clone()];
        names.extend(crate::triple::object_name(t).map(str::to_string));
        for name in names {
            let n = crate::text::normalise_name(&name);
            let named =
                |e: &Entity| crate::text::names_norm(&e.canonical_name, &e.aliases).contains(&n);
            let found = match candidates.iter().find(|e| named(e)) {
                Some(e) => Some(e.id.clone()),
                None => match batch_entities.values().find(|e| named(e)) {
                    Some(e) => Some(e.id.clone()),
                    None => self
                        .storage
                        .find_entity_by_name(ctx, &name)
                        .await?
                        .into_iter()
                        .find(|e| named(e))
                        .map(|e| e.id),
                },
            };
            match found {
                Some(id) => {
                    by_name.insert(n, id);
                }
                None => return Ok(None),
            }
        }
        Ok(crate::write_pipeline::triple_key_of(t, &by_name))
    }

    /// SA-TRIPLE-12: each admitted live single-valued triple fact of an import records its conflicts
    /// by the write's rule (write-pipeline step 8″.4), after the whole batch is stored, so two
    /// disagreeing facts of one batch are recorded too. An import ends no fact.
    // spec: components/service-layer#SA-TRIPLE-12
    async fn record_import_conflicts(
        &self,
        ctx: &ScopeContext,
        admitted: &[String],
    ) -> Result<(), AppError> {
        for id in admitted {
            let Some(fact) = self.storage.get_fact(ctx, id).await? else {
                continue;
            };
            let Some(key) = fact.triple_key.as_ref().filter(|_| fact.single_valued) else {
                continue;
            };
            if fact.valid_to.is_some() {
                continue;
            }
            let live = self
                .storage
                .live_same_subject_predicate(
                    ctx,
                    &fact.owner,
                    &key.subject_entity,
                    &key.predicate,
                    crate::store::graph::SAME_SUBJECT_PREDICATE_MAX,
                )
                .await?;
            let now = Utc::now();
            let differing = live.iter().filter(|f| {
                f.id != fact.id
                    && f.triple_key.as_ref().is_some_and(|k| {
                        k.object_kind != key.object_kind
                            || k.object_value != key.object_value
                            || k.object_unit != key.object_unit
                    })
            });
            for other in differing.take(16) {
                self.storage
                    .record_conflict(ctx, &fact.id, &other.id, now)
                    .await?;
            }
        }
        Ok(())
    }

    // --- Entities, links and graph operations (ADR-021; ordin change M-9 h) -----------------------

    /// A fact's versions, oldest first, each with the links it asserted or ended. Read class; any
    /// client.
    pub async fn fact_history(
        &self,
        cx: CallContext<'_>,
        req: FactHistoryRequest,
    ) -> Result<CallResult<FactHistory>, CallError> {
        let pipe = self
            .begin(&cx, "fact_history", Op::Read, OpClass::Read)
            .await?;
        let r = async {
            if self.storage.get_fact(&pipe.ctx, &req.id).await?.is_none() {
                return Err(AppError::NotFound);
            }
            let facts = self
                .storage
                .fact_versions(&pipe.ctx, &req.id, crate::store::graph::FACT_VERSIONS_MAX)
                .await?;
            let mut versions = Vec::with_capacity(facts.len());
            for fact in facts {
                let links = self.storage.links_of_fact(&pipe.ctx, &fact.id).await?;
                versions.push(FactVersion { fact, links });
            }
            Ok(FactHistory { versions })
        }
        .await;
        self.finish(pipe, r).await
    }

    /// The facts a summary was made from: each readable one, and whether it is no longer valid; an
    /// unreadable one as ended. Read class; any client.
    pub async fn fact_origins(
        &self,
        cx: CallContext<'_>,
        req: FactOriginsRequest,
    ) -> Result<CallResult<Vec<OriginFact>>, CallError> {
        let pipe = self
            .begin(&cx, "fact_origins", Op::Read, OpClass::Read)
            .await?;
        let r = async {
            let Some(summary) = self.storage.get_fact(&pipe.ctx, &req.id).await? else {
                return Err(AppError::NotFound);
            };
            let mut out = Vec::with_capacity(summary.derived_from.len());
            for id in &summary.derived_from {
                let fact = self.storage.get_fact(&pipe.ctx, id).await?;
                out.push(OriginFact {
                    id: id.clone(),
                    ended: fact.as_ref().is_none_or(|f| f.valid_to.is_some()),
                    fact,
                });
            }
            Ok(out)
        }
        .await;
        self.finish(pipe, r).await
    }

    /// The readable live facts recorded in conflict with each of 1..=100 facts, at most 8 each,
    /// smaller id first; an id with no row is returned with `with: []`. Read class; any client
    /// (service-layer amendment ADR-022).
    // spec: components/service-layer#SA-TRIPLE-13
    pub async fn fact_conflicts(
        &self,
        cx: CallContext<'_>,
        req: FactConflictsRequest,
    ) -> Result<CallResult<FactConflictsResult>, CallError> {
        let pipe = self
            .begin(&cx, "fact_conflicts", Op::Read, OpClass::Read)
            .await?;
        let r = async {
            let well_formed = |id: &String| {
                id.strip_prefix("fact:")
                    .is_some_and(|u| Uuid::parse_str(u).is_ok())
            };
            if req.ids.is_empty() || req.ids.len() > 100 || !req.ids.iter().all(well_formed) {
                return Err(AppError::Validation(
                    ValidationKind::OutOfRange,
                    "ids".into(),
                ));
            }
            let rows = self.storage.conflicts_of(&pipe.ctx, &req.ids).await?;
            Ok(FactConflictsResult {
                items: conflict_items(&req.ids, &rows),
            })
        }
        .await;
        self.finish(pipe, r).await
    }

    /// A page of the caller's entities by canonical name, with their counts. Read class; any client.
    pub async fn list_entities(
        &self,
        cx: CallContext<'_>,
        req: ListEntitiesRequest,
    ) -> Result<CallResult<EntityPage>, CallError> {
        let pipe = self
            .begin(&cx, "list_entities", Op::Read, OpClass::Read)
            .await?;
        let r = async {
            let out_of_range =
                |what: &str| AppError::Validation(ValidationKind::OutOfRange, what.into());
            if let Some(s) = &req.search {
                let n = s.chars().count();
                if n == 0 || n > crate::store::graph::SEARCH_MAX_CHARS {
                    return Err(out_of_range("search"));
                }
            }
            if let Some(c) = &req.cursor {
                crate::store::graph::EntityCursor::decode(c).map_err(|_| out_of_range("cursor"))?;
            }
            Ok(self
                .storage
                .list_entities(
                    &pipe.ctx,
                    req.search.as_deref(),
                    req.cursor.as_deref(),
                    crate::store::graph::ENTITY_PAGE_MAX,
                )
                .await?)
        }
        .await;
        self.finish(pipe, r).await
    }

    /// One entity with its current facts and its links, live and ended. Read class; any client.
    pub async fn entity_detail(
        &self,
        cx: CallContext<'_>,
        req: EntityDetailRequest,
    ) -> Result<CallResult<EntityDetail>, CallError> {
        let pipe = self
            .begin(&cx, "entity_detail", Op::Read, OpClass::Read)
            .await?;
        let r = async {
            self.storage
                .entity_detail(&pipe.ctx, &req.id, crate::store::graph::ENTITY_DETAIL_MAX)
                .await?
                .ok_or(AppError::NotFound)
        }
        .await;
        self.finish(pipe, r).await
    }

    /// Keep a fact from fading (person action; `ordin-in-process` only, SA-GRAPH-09).
    pub async fn keep_fact(
        &self,
        cx: CallContext<'_>,
        id: &str,
    ) -> Result<CallResult<()>, CallError> {
        let floor = self.config.salience_floor;
        self.person_write(cx, "keep_fact", Op::Write, |s, ctx| async move {
            s.storage.keep_fact(&ctx, id, floor).await?;
            Ok(())
        })
        .await
    }

    /// What forgetting an entity would remove (person action; read class).
    pub async fn count_entity(
        &self,
        cx: CallContext<'_>,
        id: &str,
    ) -> Result<CallResult<EntityCount>, CallError> {
        let pipe = self
            .begin(&cx, "count_entity", Op::Read, OpClass::Read)
            .await?;
        if !is_in_process(&pipe.ctx) {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::InvalidBody, "count_entity".into()),
                )
                .await;
        }
        let r = self
            .storage
            .count_entity(&pipe.ctx, id)
            .await
            .map_err(store_error);
        self.finish(pipe, r).await
    }

    /// Forget everything about an entity, with proof (person action; forget class).
    pub async fn forget_entity(
        &self,
        cx: CallContext<'_>,
        id: &str,
    ) -> Result<CallResult<EntityErasureProof>, CallError> {
        self.person_write(cx, "forget_entity", Op::Forget, |s, ctx| async move {
            Ok(s.storage.erase_entity(&ctx, id).await?)
        })
        .await
    }

    /// Fold `merge` into `keep` (person action).
    pub async fn merge_entities(
        &self,
        cx: CallContext<'_>,
        keep: &str,
        merge: &str,
    ) -> Result<CallResult<()>, CallError> {
        self.person_write(cx, "merge_entities", Op::Write, |s, ctx| async move {
            if keep == merge {
                return Err(AppError::Validation(
                    ValidationKind::InvalidBody,
                    "merge".into(),
                ));
            }
            s.storage.merge_entities(&ctx, keep, merge).await?;
            Ok(())
        })
        .await
    }

    /// Replace a link with the person's correction: the old link ends now, and a new one, recording
    /// what it `replaces`, starts (person action).
    // spec: components/memory-store#SA-GRAPH-05
    pub async fn correct_link(
        &self,
        cx: CallContext<'_>,
        req: CorrectLinkRequest,
    ) -> Result<CallResult<Relationship>, CallError> {
        let key = cx.idempotency_key.unwrap_or("").to_string();
        self.person_write(cx, "correct_link", Op::Write, |s, ctx| async move {
            let Some(old) = s.storage.get_relationship(&ctx, &req.id).await? else {
                return Err(AppError::NotFound);
            };
            if old.owner.user != ctx.user {
                return Err(AppError::Store(StoreError::ScopeForbidden));
            }
            // The correction reaches whoever could read what it corrects, and no one else.
            let owner_only = !s.storage.link_is_shared(&ctx, &req.id).await?;
            for e in [&req.from, &req.to] {
                if s.storage.get_entity(&ctx, e).await?.is_none() {
                    return Err(AppError::NotFound);
                }
            }
            let kind = crate::text::normalise_kind(&req.kind);
            if kind.is_empty() || kind.chars().count() > 128 {
                return Err(AppError::Validation(
                    ValidationKind::InvalidBody,
                    "kind".into(),
                ));
            }
            let now = Utc::now();
            s.storage
                .end_relationship_validity(&ctx, &req.id, now)
                .await?;
            let rel = Relationship {
                id: format!(
                    "relationship:{}",
                    Uuid::new_v5(
                        &Uuid::NAMESPACE_DNS,
                        format!("relationship:correct:{}:{key}", req.id).as_bytes()
                    )
                ),
                kind,
                from: req.from.clone(),
                to: req.to.clone(),
                valid_from: now,
                valid_to: None,
                ingested_at: now,
                confidence: 1.0,
                source_id: None,
                owner: old.owner.clone(),
                collection: old.collection.clone(),
                replaces: Some(req.id.clone()),
                owner_only,
                author: None,
            };
            s.storage.put_relationship(&rel).await?;
            Ok(rel)
        })
        .await
    }

    /// Delete one link, with proof (person action; forget class).
    pub async fn forget_link(
        &self,
        cx: CallContext<'_>,
        id: &str,
    ) -> Result<CallResult<DeletionProof>, CallError> {
        self.person_write(cx, "forget_link", Op::Forget, |s, ctx| async move {
            Ok(s.storage.delete_relationship(&ctx, id).await?)
        })
        .await
    }

    /// The chain of a person write (SA-GRAPH-09): authenticate, authorise, rate-limit, refuse any
    /// caller but the in-process ordin client, require and replay the idempotency key (route = the
    /// operation name), run `body`, persist its outcome, audit.
    async fn person_write<'s, T, F, Fut>(
        &'s self,
        cx: CallContext<'_>,
        operation: &'static str,
        op: Op,
        body: F,
    ) -> Result<CallResult<T>, CallError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce(&'s Service, ScopeContext) -> Fut,
        Fut: std::future::Future<Output = Result<T, AppError>>,
    {
        let pipe = self.begin(&cx, operation, op, OpClass::Write).await?;
        if !is_in_process(&pipe.ctx) {
            return self
                .finish_error(
                    pipe,
                    AppError::Validation(ValidationKind::InvalidBody, operation.into()),
                )
                .await;
        }
        let key = match Self::require_idempotency_key(&cx) {
            Ok(k) => k,
            Err(e) => return self.finish_error(pipe, e).await,
        };
        match self.idempotency_replay::<T>(&pipe, operation, &key).await {
            Ok(Some(data)) => return self.finish_replay(pipe, data).await,
            Ok(None) => {}
            Err(e) => return self.finish_error(pipe, e).await,
        }
        let data = match body(self, pipe.ctx.clone()).await {
            Ok(d) => d,
            Err(e) => return self.finish_error(pipe, e).await,
        };
        let stored = serde_json::json!({
            "data": serde_json::to_value(&data).unwrap_or(Value::Null),
            "meta": { "correlation_id": pipe.ctx.correlation_id }
        });
        if let Err(e) = self
            .idempotency_persist(&pipe, operation, &key, 200, &stored)
            .await
        {
            return self.finish_error(pipe, e).await;
        }
        self.finish_success(pipe, data).await
    }

    /// Finish a call with the body's result: success, or the error audited.
    async fn finish<T>(
        &self,
        pipe: Authorised,
        r: Result<T, AppError>,
    ) -> Result<CallResult<T>, CallError> {
        match r {
            Ok(d) => self.finish_success(pipe, d).await,
            Err(e) => self.finish_error(pipe, e).await,
        }
    }

    /// Step 5 of `remember` (ADR-021): `entities`, `links` and `derived_from` within their bounds, every
    /// link endpoint named by the request, and every origin readable and valid.
    async fn validate_graph_fields(
        &self,
        ctx: &ScopeContext,
        req: &RememberRequest,
    ) -> Result<(), AppError> {
        let bad = |path: String| AppError::Validation(ValidationKind::InvalidBody, path);
        let name_ok = |n: &str| (1..=512).contains(&n.trim().chars().count());
        if req.entities.len() > 16 {
            return Err(bad("entities".into()));
        }
        let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (i, e) in req.entities.iter().enumerate() {
            if !name_ok(&e.name) {
                return Err(bad(format!("entities[{i}].name")));
            }
            if e.aliases.len() > 32 {
                return Err(bad(format!("entities[{i}].aliases")));
            }
            for (j, a) in e.aliases.iter().enumerate() {
                if !name_ok(a) {
                    return Err(bad(format!("entities[{i}].aliases[{j}]")));
                }
                names.insert(crate::text::normalise_name(a));
            }
            names.insert(crate::text::normalise_name(&e.name));
        }
        for key in ["subject", "object"] {
            if let Some(v) = req.content.get(key).and_then(|v| v.as_str()) {
                names.insert(crate::text::normalise_name(v));
            }
        }
        // ADR-022: an entity object's name is `content.object.name`.
        // spec: phase-1-analysis#SA-TRIPLE-04
        if let Some(v) = req
            .content
            .get("object")
            .filter(|o| o.get("kind").and_then(|k| k.as_str()) == Some("entity"))
            .and_then(|o| o.get("name"))
            .and_then(|v| v.as_str())
        {
            names.insert(crate::text::normalise_name(v));
        }
        names.remove("");
        if req.links.len() > 16 {
            return Err(bad("links".into()));
        }
        for (i, l) in req.links.iter().enumerate() {
            let k = crate::text::normalise_kind(&l.kind).chars().count();
            if !(1..=128).contains(&k) {
                return Err(bad(format!("links[{i}].kind")));
            }
            if l.confidence.is_some_and(|c| !(0.0..=1.0).contains(&c)) {
                return Err(bad(format!("links[{i}].confidence")));
            }
            if !names.contains(&crate::text::normalise_name(&l.from)) {
                return Err(bad(format!("links[{i}].from")));
            }
            if !names.contains(&crate::text::normalise_name(&l.to)) {
                return Err(bad(format!("links[{i}].to")));
            }
        }
        if !req.derived_from.is_empty() {
            if req.memory_class != Some(MemoryClass::Consolidated) {
                return Err(bad("derived_from".into()));
            }
            let distinct: std::collections::HashSet<&String> = req.derived_from.iter().collect();
            if !(2..=64).contains(&req.derived_from.len())
                || distinct.len() != req.derived_from.len()
            {
                return Err(bad("derived_from".into()));
            }
            for (i, id) in req.derived_from.iter().enumerate() {
                let well_formed = id
                    .strip_prefix("fact:")
                    .is_some_and(|u| Uuid::parse_str(u).is_ok());
                if !well_formed {
                    return Err(bad(format!("derived_from[{i}]")));
                }
                let live = self
                    .storage
                    .get_fact(ctx, id)
                    .await?
                    .is_some_and(|f| f.valid_to.is_none());
                if !live {
                    return Err(bad(format!("derived_from[{i}]")));
                }
            }
        }
        Ok(())
    }

    // --- Orchestration helpers (the fixed sequence, defined once) ---------------------------------

    /// Internal Logic steps 1–3: authenticate (C3), authorise the operation class, and decrement the
    /// `(subject, op-class)` rate bucket. On any failure no audit record is written (the call either
    /// never reached an authenticated identity or is an unauthenticated-class outcome, matching the
    /// existing Gherkin) and the typed error is returned. On a 429 the [`RateSnapshot`] is still
    /// produced (carried on the returned error path's rate via the edge), so the edge can attach limit
    /// signals.
    async fn begin(
        &self,
        cx: &CallContext<'_>,
        operation: &'static str,
        op: Op,
        class: OpClass,
    ) -> Result<Authorised, CallError> {
        // Step 1 — authenticate. No rate snapshot accompanies an auth failure (the bucket is untouched),
        // matching the C8 behaviour where a 401 carries no `RateLimit-*` headers.
        let ctx = match self.auth.validate(cx.bearer, cx.correlation_id).await {
            Ok(ctx) => ctx,
            Err(e) => {
                let app = match e {
                    crate::auth::AuthError::MissingToken => {
                        AppError::Unauthenticated(AuthKind::Missing, String::new())
                    }
                    crate::auth::AuthError::InvalidToken(detail) => {
                        AppError::Unauthenticated(AuthKind::Invalid, detail)
                    }
                    // validate() never returns InsufficientScope; treat defensively as invalid.
                    crate::auth::AuthError::InsufficientScope(_) => {
                        AppError::Unauthenticated(AuthKind::Invalid, String::new())
                    }
                };
                return Err(app.into());
            }
        };

        // Step 2 — authorise the operation class against the token scopes. A 403 also carries no rate
        // snapshot (the bucket is untouched), matching the C8 behaviour.
        if let Err(crate::auth::AuthError::InsufficientScope(_)) = crate::auth::authorise(&ctx, op)
        {
            return Err(AppError::InsufficientScope(format!("{op:?}")).into());
        }

        // Step 3 — rate limit, keyed by (subject, op-class). On exhaustion the snapshot is still
        // produced so the edge can attach `RateLimit-*` and `Retry-After` to the 429.
        let rate = match self.decrement_rate(&ctx.user, class).await {
            Ok(h) => h,
            Err(h) => {
                return Err(CallError {
                    error: AppError::RateLimited,
                    rate: Some(h.into()),
                })
            }
        };

        Ok(Authorised {
            ctx,
            operation,
            rate,
            // Set by `remember` once the body is parsed; `None` on every other operation, which is
            // also what the audit row records for them.
            fact_class: None,
        })
    }

    /// Decrement the `(subject, class)` token bucket, creating a full bucket on first use. Returns the
    /// rate headers (`Ok` allowed, `Err` rejected).
    async fn decrement_rate(&self, user: &str, class: OpClass) -> Result<RateHeaders, RateHeaders> {
        let (per_min, burst) = match class {
            OpClass::Read => (self.config.rate_read_per_min, READ_BURST),
            OpClass::Write => (self.config.rate_write_per_min, WRITE_BURST),
        };
        let mut map = self.rate.lock().await;
        let bucket = map
            .entry((user.to_string(), class))
            .or_insert_with(|| TokenBucket::new(burst, per_min));
        bucket.take(std::time::Instant::now())
    }

    /// Finish a successful call: write the audit record (step 6) before returning. An audit-write
    /// failure turns the success into a `503 STORE_UNAVAILABLE` (the call is not reported successful if
    /// it could not be audited).
    async fn finish_success<T>(
        &self,
        pipe: Authorised,
        data: T,
    ) -> Result<CallResult<T>, CallError> {
        // An audit-write failure on the success path is itself a store error. The C8 `finish_success`
        // returned the bare error response for this case (no `RateLimit-*` headers attached), so the
        // resulting 503 carries no rate snapshot.
        if let Err(error) = self.write_audit(&pipe, "success").await {
            return Err(CallError { error, rate: None });
        }
        Ok(CallResult {
            data,
            rate: pipe.rate.into(),
            replayed: false,
        })
    }

    /// Finish an idempotent replay: return the replayed data with `replayed=true` and the rate snapshot,
    /// performing **no** new side effect. The C8 handler returned the stored response verbatim on a hit
    /// (with `RateLimit-*` attached) and wrote **no** audit record for the replay, so this path writes
    /// no audit record either.
    async fn finish_replay<T>(
        &self,
        pipe: Authorised,
        data: T,
    ) -> Result<CallResult<T>, CallError> {
        Ok(CallResult {
            data,
            rate: pipe.rate.into(),
            replayed: true,
        })
    }

    /// Finish an error call after auth succeeded (step 6, error path): best-effort audit of the failed
    /// outcome — an audit failure never masks the original error. Returns the original `AppError` with
    /// the rate snapshot attached so the edge can still emit `RateLimit-*`.
    async fn finish_error<T>(
        &self,
        pipe: Authorised,
        err: AppError,
    ) -> Result<CallResult<T>, CallError> {
        let code = err.code();
        // Best-effort audit; the handler already failed, so surface the original error (do not mask it).
        let _ = self.write_audit(&pipe, code).await;
        // The C8 `finish_error` attached `RateLimit-*` to a post-auth error, so the snapshot rides along.
        Err(CallError {
            error: err,
            rate: Some(pipe.rate.into()),
        })
    }

    /// Internal Logic step 6 — write one append-only audit record to the per-tenant `audit_log` table.
    /// The record never contains the token, request payload, or fact content (SA-AUDIT-01).
    async fn write_audit(&self, pipe: &Authorised, outcome: &str) -> Result<(), AppError> {
        self.write_audit_as(pipe, pipe.operation, outcome).await
    }

    /// One audit record of `operation` for the authorised call — the call's own record, or a record
    /// per admitted fact of an import (operation `import`, ordin change M-7 b).
    async fn write_audit_as(
        &self,
        pipe: &Authorised,
        operation: &str,
        outcome: &str,
    ) -> Result<(), AppError> {
        let entry = AuditEntry {
            id: format!("audit_log:{}", Uuid::now_v7()),
            tenant: pipe.ctx.tenant.clone(),
            subject: pipe.ctx.user.clone(),
            operation: operation.to_string(),
            scope: ScopeRef::from(&pipe.ctx),
            outcome: outcome.to_string(),
            token_jti: pipe.ctx.token_jti.clone(),
            correlation_id: pipe.ctx.correlation_id.clone(),
            client: pipe.ctx.client.clone(),
            fact_class: pipe.fact_class.clone(),
            at: Utc::now(),
        };
        self.storage
            .append_audit(&entry)
            .await
            .map_err(AppError::Store)
    }

    // --- Idempotency (chain step 4, writes only) --------------------------------------------------

    /// Read + validate the `idempotency_key` for a write call: present, non-empty, 1..=255 chars.
    fn require_idempotency_key(cx: &CallContext<'_>) -> Result<String, AppError> {
        let key = cx.idempotency_key.unwrap_or("");
        if key.is_empty() || key.chars().count() > 255 {
            return Err(AppError::Validation(
                ValidationKind::MissingIdempotencyKey,
                "Idempotency-Key header is required for writes".into(),
            ));
        }
        Ok(key.to_string())
    }

    /// Look up a stored idempotency outcome for a write route. On a non-expired hit, deserialises the
    /// stored envelope's `data` sub-object into the typed payload `T`. On a miss returns `Ok(None)`. A
    /// store failure is surfaced as `AppError::Store`.
    async fn idempotency_replay<T: DeserializeOwned>(
        &self,
        pipe: &Authorised,
        route: &str,
        key: &str,
    ) -> Result<Option<T>, AppError> {
        match self
            .storage
            .idempotency_get(&pipe.ctx.tenant, &pipe.ctx.user, route, key)
            .await
        {
            Ok(Some(rec)) => {
                // The stored body is a `Success`-shaped envelope; the typed payload is `data`.
                let data = rec
                    .response_body
                    .get("data")
                    .cloned()
                    .ok_or(AppError::Internal)
                    .and_then(|d| serde_json::from_value::<T>(d).map_err(|_| AppError::Internal))?;
                Ok(Some(data))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AppError::Store(e)),
        }
    }

    /// Persist a write call's produced outcome under the idempotency key (TTL =
    /// `MEMORY_IDEMPOTENCY_TTL_SECS`).
    async fn idempotency_persist(
        &self,
        pipe: &Authorised,
        route: &str,
        key: &str,
        status_code: u16,
        body: &Value,
    ) -> Result<(), AppError> {
        let created_at = Utc::now();
        let rec = IdempotencyRecord {
            tenant: pipe.ctx.tenant.clone(),
            user: pipe.ctx.user.clone(),
            route: route.to_string(),
            idempotency_key: key.to_string(),
            response_body: body.clone(),
            status_code,
            created_at,
            expires_at: created_at + Duration::seconds(i64::from(self.config.idempotency_ttl_secs)),
        };
        self.storage
            .idempotency_put(&rec)
            .await
            .map_err(AppError::Store)
    }
}

/// Build a `ScopeRef` from an authenticated context: the team is the first team membership (or none).
impl From<&ScopeContext> for ScopeRef {
    fn from(ctx: &ScopeContext) -> Self {
        ScopeRef {
            tenant: ctx.tenant.clone(),
            team: ctx.teams.first().cloned(),
            user: ctx.user.clone(),
        }
    }
}

/// Build the work-job payload for a remember request: the request serialised as JSON (structured
/// content + optional source + optional memory_class), validated by the C4 consumer. Content is
/// agent-asserted; recall performs no LLM extraction (ADR-015).
/// `fact_conflicts`' items: one per requested id, in request order, each `with` ordered by id and cut
/// to 8 (service-layer amendment ADR-022). Shared with recall's `RankedFact.conflicts`.
// spec: phase-1-analysis#SA-TRIPLE-13
pub(crate) fn conflict_items(
    ids: &[String],
    rows: &[crate::types::ports::FactConflict],
) -> Vec<FactConflictItem> {
    let mut seen = std::collections::HashSet::new();
    ids.iter()
        .filter(|id| seen.insert(id.as_str()))
        .map(|id| {
            let mut with: Vec<String> = rows
                .iter()
                .filter(|r| &r.fact == id)
                .map(|r| r.with.clone())
                .collect();
            with.sort();
            with.dedup();
            with.truncate(8);
            FactConflictItem {
                fact: id.clone(),
                with,
            }
        })
        .collect()
}

/// Step 5 of `remember` (ADR-022): a non-passage write's content is one triple whose parts and object
/// value are within their bounds (SA-TRIPLE-01, SA-TRIPLE-02); the flags agree (SA-TRIPLE-05) and none
/// is set on a passage (SA-TRIPLE-11); `confidence` is a finite number in [0,1] and `valid_from` is not
/// later than `now` (SA-TRIPLE-14). Each refusal names its field.
// spec: components/service-layer#SA-TRIPLE-05
fn check_triple_write(
    req: &RememberRequest,
    passage_class: &str,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let bad = |field: &str| AppError::Validation(ValidationKind::InvalidBody, field.to_string());
    if req.fact_class.as_deref() == Some(passage_class) {
        // spec: phase-1-analysis#SA-TRIPLE-11
        for (flag, set) in [
            ("single_valued", req.single_valued),
            ("changed", req.changed),
            ("ended", req.ended),
        ] {
            if set {
                return Err(bad(flag));
            }
        }
    } else {
        crate::triple::check_triple(&req.content).map_err(bad)?;
        if req.changed && !req.single_valued {
            return Err(bad("changed"));
        }
        if req.changed && req.ended {
            return Err(bad("ended"));
        }
    }
    if req
        .confidence
        .is_some_and(|c| !c.is_finite() || !(0.0..=1.0).contains(&c))
    {
        return Err(bad("confidence"));
    }
    if req.valid_from.is_some_and(|at| at > now) {
        return Err(bad("valid_from"));
    }
    Ok(())
}

fn remember_payload(req: &RememberRequest) -> Value {
    let mut payload = serde_json::json!({
        "content": req.content,
    });
    if let Some(mc) = &req.memory_class {
        payload["memory_class"] = serde_json::to_value(mc).unwrap_or(Value::Null);
    }
    // The class the authority check above just admitted travels with the job, so the label the gate
    // judged is the label C4 persists. Dropping it here is what let the judged label and the
    // read-back label diverge before RFC 05 — a caller could pass the gate on one class while its
    // own readers acted on another inside `content`, which memory never compares against anything.
    // Absent stays absent: an unclassed write carries no key rather than a null.
    // spec: phase-1-analysis#SA-AUTHZ-09
    if let Some(fc) = &req.fact_class {
        payload["fact_class"] = Value::String(fc.clone());
    }
    // Only ever present for the in-process ordin caller: `remember` refused it from anyone else.
    if let Some(v) = &req.visibility {
        payload["visibility"] = serde_json::to_value(v).unwrap_or(Value::Null);
    }
    // Validated by `remember` (SA-COLL-01); absent stays absent and C4 reads it as "".
    if let Some(c) = &req.collection {
        payload["collection"] = Value::String(c.clone());
    }
    // ADR-021: validated by `remember`; absent stays absent.
    if !req.entities.is_empty() {
        payload["entities"] = serde_json::to_value(&req.entities).unwrap_or(Value::Null);
    }
    if !req.links.is_empty() {
        payload["links"] = serde_json::to_value(&req.links).unwrap_or(Value::Null);
    }
    if !req.derived_from.is_empty() {
        payload["derived_from"] = serde_json::to_value(&req.derived_from).unwrap_or(Value::Null);
    }
    // Only ever present for the in-process ordin caller: `remember` refused them from anyone else.
    if let Some(trust) = req.gate_trust {
        payload["gate_trust"] = Value::from(trust);
    }
    if let Some(author) = &req.author {
        payload["author"] = Value::String(author.clone());
    }
    // ADR-022: the writer's statements, validated by `remember`; false and absent stay absent.
    // spec: phase-1-analysis#SA-TRIPLE-05
    for (flag, set) in [
        ("single_valued", req.single_valued),
        ("changed", req.changed),
        ("ended", req.ended),
    ] {
        if set {
            payload[flag] = Value::Bool(true);
        }
    }
    // spec: phase-1-analysis#SA-TRIPLE-14
    if let Some(c) = req.confidence {
        payload["confidence"] = Value::from(c);
    }
    if let Some(at) = req.valid_from {
        payload["valid_from"] = serde_json::to_value(at).unwrap_or(Value::Null);
    }
    if let Some(src) = &req.source {
        let mut s = serde_json::json!({ "origin_ref": src.origin_ref });
        if let Some(m) = &src.modification_marker {
            s["modification_marker"] = Value::String(m.clone());
        }
        payload["source"] = s;
    }
    payload
}

/// Build the stored idempotency body for a remember replay: the `Success<WriteAck>` shape carrying the
/// original `job_id` and `status = already-accepted` (so a replay deserialises back to
/// `JobAckStatus::AlreadyAccepted`).
fn remember_stored_body(correlation_id: &str, job_id: &str) -> Value {
    serde_json::json!({
        "data": { "job_id": job_id, "status": "already-accepted" },
        "meta": { "correlation_id": correlation_id }
    })
}

/// A store error as the Service reports it: an absent or unreadable record is `NotFound`, an
/// unverified deletion is `Internal` (never reported complete, SA-DELETE-01).
fn store_error(e: StoreError) -> AppError {
    match e {
        StoreError::NotFound => AppError::NotFound,
        StoreError::PartialDelete { .. } => AppError::Internal,
        other => AppError::Store(other),
    }
}

/// The folder records of an imported entity. A re-import never widens a name's folders.
fn imported_alias_folders(e: &Entity, stored: Option<&Entity>) -> Vec<AliasFolder> {
    let mut out: Vec<AliasFolder> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for alias in e.aliases.iter().map(|a| crate::text::normalise_name(a)) {
        if !seen.insert(alias.clone()) {
            continue;
        }
        match stored {
            Some(s)
                if s.aliases
                    .iter()
                    .any(|a| crate::text::normalise_name(a) == alias) =>
            {
                out.extend(s.alias_folders.iter().filter(|f| f.alias == alias).cloned())
            }
            _ => out.push(AliasFolder {
                alias,
                team: e.owner.team.clone(),
                collection: e.collection.clone(),
            }),
        }
    }
    out
}

/// Whether `ctx` is the in-process ordin caller (ordin changes M-6 a, M-7 b and c).
fn is_in_process(ctx: &ScopeContext) -> bool {
    ctx.client.as_deref() == Some(IN_PROCESS_CLIENT)
}

/// Whether `owner` is the caller's own scope: its tenant and user, and no team or one of its teams.
/// An imported item is written only under it.
fn owned_by_caller(ctx: &ScopeContext, owner: &ScopeRef) -> bool {
    owner.tenant == ctx.tenant
        && owner.user == ctx.user
        && owner.team.as_ref().is_none_or(|t| ctx.teams.contains(t))
}

/// Whether an imported `id` names a record of the `prefix` table. A record store takes the table from
/// the id, so a companion item's id is checked as a fact's is.
// Another table's prefix would plant a row a tenant-wide read cannot decode.
// SurrealDB's id normalisation trims and strips quoting characters, so `fact:<id> ` would be a new
// record that read back as another user's `fact:<id>`, past the owner check. Only characters every
// store returns unchanged are taken.
fn is_record_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix).is_some_and(|key| {
        !key.is_empty()
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

fn invalid(reason: &str) -> ImportResult {
    ImportResult::Invalid {
        reason: reason.to_string(),
    }
}

/// The first structural defect of an imported fact, as the reason `Invalid` carries, or `None`. The
/// rules are the store's own (`validate_fact`) plus the id's table and the caller's ownership, checked
/// here so one bad fact is reported against its id instead of failing the call.
fn import_fact_defect(ctx: &ScopeContext, f: &Fact) -> Option<&'static str> {
    if !is_record_id(&f.id, "fact:") {
        return Some("id");
    }
    // A malformed one would fail every later history read.
    let refers_to_facts = f
        .supersedes
        .iter()
        .chain(&f.superseded_by)
        .chain(&f.derived_from)
        .all(|id| is_record_id(id, "fact:"));
    if !refers_to_facts {
        return Some("fact references");
    }
    if !owned_by_caller(ctx, &f.owner) {
        return Some("owner");
    }
    if !f.content.is_object() {
        return Some("content");
    }
    if f.entities.is_empty() {
        return Some("entities");
    }
    if !(0.0..=1.0).contains(&f.confidence) {
        return Some("confidence");
    }
    if !(0.0..=1.0).contains(&f.salience) || f.stability < 0.0 {
        return Some("usage fields");
    }
    if f.valid_to.is_some_and(|to| to < f.valid_from) {
        return Some("valid_to");
    }
    if !crate::types::domain::is_valid_collection(&f.collection) {
        return Some("collection");
    }
    None
}

/// What an imported fact's confidence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfidenceIs {
    /// What the file says; its source's trust has not been applied here.
    Stated,
    /// What this store computed.
    Scored,
}

/// Whether an imported fact equals the stored fact of its id in every field the import carries.
/// Usage fields (salience, stability, `last_recalled_at`, `ingested_at`) and `pii_review` are not
/// carried. `content` matches either as given or as the personal-data scan redacts it, which is the
/// form the earlier import stored; instants compare to the millisecond every backend keeps.
fn carried_fields_equal(
    stored: &Fact,
    imported: &Fact,
    redacted: &Value,
    stated_confidence: f64,
) -> bool {
    let same_instant =
        |a: DateTime<Utc>, b: DateTime<Utc>| a.timestamp_millis() == b.timestamp_millis();
    let same_opt_instant = |a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>| match (a, b) {
        (Some(a), Some(b)) => same_instant(a, b),
        (None, None) => true,
        _ => false,
    };
    (stored.content == imported.content || stored.content == *redacted)
        && stored.entities == imported.entities
        && stored.source_id == imported.source_id
        && stored.memory_class == imported.memory_class
        && stored.fact_class == imported.fact_class
        && stored.visibility == imported.visibility
        && stored.owner == imported.owner
        && same_instant(stored.valid_from, imported.valid_from)
        && same_opt_instant(stored.valid_to, imported.valid_to)
        && (stored.confidence == imported.confidence || stored.confidence == stated_confidence)
        && stored.supersedes == imported.supersedes
        && stored.superseded_by == imported.superseded_by
        && stored.derived_from == imported.derived_from
        && stored.collection == imported.collection
}

/// Encode a cursor payload as base64url(JSON), as the recall cursor is (SA-PAGE-01).
fn encode_cursor<T: Serialize>(payload: &T) -> Result<String, AppError> {
    let json = serde_json::to_vec(payload).map_err(|_| AppError::Internal)?;
    Ok(URL_SAFE_NO_PAD.encode(json))
}

/// Decode a cursor `encode_cursor` produced; anything else is `VAL_OUT_OF_RANGE`, as for recall.
fn decode_cursor<T: DeserializeOwned>(raw: &str) -> Result<T, AppError> {
    let bytes = URL_SAFE_NO_PAD.decode(raw.as_bytes()).map_err(|_| {
        AppError::Validation(ValidationKind::OutOfRange, "undecodable cursor".into())
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Validation(ValidationKind::OutOfRange, "malformed cursor".into()))
}

/// The `list_facts` keyset: `(ingested_at, id)` of the last fact of the page. Never persisted.
#[derive(Serialize, Deserialize)]
struct ListCursor {
    at: DateTime<Utc>,
    id: String,
}

impl ListCursor {
    fn encode(&self) -> Result<String, AppError> {
        encode_cursor(self)
    }

    fn decode(raw: &str) -> Result<Self, AppError> {
        decode_cursor(raw)
    }
}

/// The `export_scope` keyset: the id of the last fact of the page. Never persisted.
#[derive(Serialize, Deserialize)]
struct ExportCursor {
    id: String,
}

impl ExportCursor {
    fn encode(&self) -> Result<String, AppError> {
        encode_cursor(self)
    }

    fn decode(raw: &str) -> Result<Self, AppError> {
        decode_cursor(raw)
    }
}

#[cfg(test)]
mod tests {
    //! Supportive unit tests: the chain orchestration is exercised through the public methods with no
    //! axum or HTTP type in scope. The BDD suite is the behaviour-preservation gate.

    use super::*;

    #[test]
    fn rate_snapshot_mirrors_rate_headers() {
        let h = RateHeaders {
            limit: 120,
            remaining: 41,
            reset_secs: 7,
        };
        let snap: RateSnapshot = h.into();
        assert_eq!(snap.limit, 120);
        assert_eq!(snap.remaining, 41);
        assert_eq!(snap.reset_secs, 7);
    }

    #[test]
    fn require_idempotency_key_rejects_missing_empty_and_overlong() {
        // Table-driven: each row is (key, expected_ok).
        let cases: &[(Option<&str>, bool)] = &[
            (None, false),
            (Some(""), false),
            (Some("k-001"), true),
            // Exactly 255 chars is the inclusive upper bound (accepted); 256 is rejected.
            (Some("x"), true),
        ];
        for (key, ok) in cases {
            let cx = CallContext {
                bearer: "t",
                correlation_id: "c",
                idempotency_key: *key,
            };
            assert_eq!(
                Service::require_idempotency_key(&cx).is_ok(),
                *ok,
                "key {key:?}"
            );
        }

        let long = "k".repeat(256);
        let cx = CallContext {
            bearer: "t",
            correlation_id: "c",
            idempotency_key: Some(&long),
        };
        assert!(Service::require_idempotency_key(&cx).is_err());

        let exact = "k".repeat(255);
        let cx = CallContext {
            bearer: "t",
            correlation_id: "c",
            idempotency_key: Some(&exact),
        };
        assert!(Service::require_idempotency_key(&cx).is_ok());
    }

    #[test]
    fn remember_stored_body_round_trips_to_already_accepted() {
        let body = remember_stored_body("corr-1", "work_job:abc");
        let data = body.get("data").cloned().unwrap();
        let ack: WriteAck = serde_json::from_value(data).unwrap();
        assert!(matches!(ack.status, JobAckStatus::AlreadyAccepted));
        assert_eq!(ack.job_id, "work_job:abc");
    }

    #[test]
    fn remember_payload_includes_optional_source_and_class() {
        let req: RememberRequest = serde_json::from_value(serde_json::json!({
            "content": {"subject": "a", "predicate": "owns", "object": "b"},
            "source": {"origin_ref": "src:1", "modification_marker": "m"},
            "memory_class": "semantic"
        }))
        .unwrap();
        let payload = remember_payload(&req);
        assert_eq!(payload["content"]["subject"], "a");
        assert_eq!(payload["source"]["origin_ref"], "src:1");
        assert_eq!(payload["source"]["modification_marker"], "m");
        assert_eq!(payload["memory_class"], "semantic");
    }

    /// The authorised class travels with the job. Without this the label the gate judged and the
    /// label a consumer reads back are different strings with nothing keeping them in step (RFC 05).
    #[test]
    fn remember_payload_carries_the_authorised_fact_class() {
        let req: RememberRequest = serde_json::from_value(serde_json::json!({
            "content": {"subject": "a", "predicate": "owns", "object": "b"},
            "fact_class": "medication-authoritative"
        }))
        .unwrap();
        let payload = remember_payload(&req);
        assert_eq!(payload["fact_class"], "medication-authoritative");
    }

    /// An unclassed write carries no key at all — not a null, not an empty string. Absent stays
    /// absent all the way to the stored row (RFC 05 acceptance criterion 3).
    #[test]
    fn remember_payload_omits_an_absent_fact_class() {
        let req: RememberRequest = serde_json::from_value(serde_json::json!({
            "content": {"subject": "a", "predicate": "owns", "object": "b"}
        }))
        .unwrap();
        let payload = remember_payload(&req);
        assert!(
            payload.get("fact_class").is_none(),
            "an unclassed write must carry no fact_class key, got {payload}"
        );
    }
}
