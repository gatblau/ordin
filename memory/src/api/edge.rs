//! The axum HTTP edge (features `http` and `surreal`); see the parent module for the chain order.

#[path = "envelope.rs"]
pub mod envelope;
#[path = "health.rs"]
pub mod health;
#[path = "openapi.rs"]
pub mod openapi;
#[path = "v1.rs"]
pub mod v1;

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use http::{HeaderValue, StatusCode};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::auth::Authenticator;
use crate::config::Config;
use crate::error::{AppError, Env};
use crate::obs::metrics::Metrics;
use crate::retrieval::RetrievalEngine;
use crate::store::surreal::Store;
use crate::types::envelope::{ErrorBody, ErrorEnvelope};

use super::ratelimit::{OpClass, TokenBucket};

/// Header carrying the per-request correlation id, echoed on the response.
pub const CORRELATION_ID_HEADER: &str = "x-correlation-id";

/// The in-process rate-limiter state: one token bucket per `(subject, operation-class)`. Bounded —
/// the only in-process state the edge holds (the idempotency record lives in C1's store).
type RateLimiter = Arc<Mutex<HashMap<(String, OpClass), TokenBucket>>>;

/// Shared application state injected into every handler. Cheap to clone (Arc-backed).
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub metrics: Arc<Metrics>,
    /// Concrete store: the whole storage port (facts, audit, idempotency, the work queue), plus
    /// `index_embed_dim` for `/readyz`.
    pub store: Arc<Store>,
    pub engine: Arc<RetrievalEngine>,
    pub auth: Arc<Authenticator>,
    /// Per-(subject, op-class) token buckets (SA-RATE-01).
    pub rate: RateLimiter,
}

impl AppState {
    /// Build a [`Service`](crate::service::Service) over this state's component handles. Cheap — every
    /// field is `Arc`-backed, so this clones handles, not data. The Service shares the **same** rate
    /// limiter map as the edge, so a test that seeds an empty bucket through the shared `rate` Arc is
    /// observed by the Service's rate check. The single definition of the auth/scope/rate/idempotency/
    /// audit chain lives in the Service (ADR-016); the handlers are thin adapters over it.
    pub fn service(&self) -> crate::service::Service {
        crate::service::Service::new(
            self.config.clone(),
            self.metrics.clone(),
            self.store.clone(),
            self.engine.clone(),
            self.auth.clone(),
            self.rate.clone(),
        )
    }

    /// Mint a fresh correlation id (UUID v4). Used by handlers reached without a middleware-assigned id.
    pub fn new_correlation_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    /// The configured deployment environment, used by the error-envelope mapping.
    pub fn env(&self) -> Env {
        self.config.env
    }
}

/// Build the production axum router: the `/v1` task routes under the middleware chain, plus the three
/// unauthenticated operational routes (`/healthz`, `/readyz`, `/metrics`).
///
/// The two outermost middleware concerns are expressed as tower layers so their order is structural:
///   1. correlation-id assignment (innermost-applied `.layer` runs *outermost*, see below)
///   2. body-size limit (`DefaultBodyLimit`)
///
/// In axum/tower, `.layer(a).layer(b)` makes `a` the outer layer. We therefore apply body-size limit
/// first then correlation-id, so correlation-id is the outermost concern (it must wrap everything so
/// even a 413 carries the id). The remaining concerns (auth → rate → idempotency → handler → audit)
/// run inside each handler in that fixed order.
pub fn build_router(state: AppState) -> Router {
    let max_body = state.config.max_body_bytes as usize;

    Router::new()
        // --- /v1 task routes (auth + rate-limit + idempotency + audit run inside the handlers) ---
        .route("/v1", get(v1::capabilities))
        .route("/v1/recall", post(v1::recall))
        .route("/v1/memories", post(v1::remember))
        // ADR-022: before `/v1/memories/:id`, so `conflicts` is never read as an id.
        .route("/v1/memories/conflicts", get(v1::fact_conflicts))
        .route("/v1/memories/:id", get(v1::get_memory))
        .route("/v1/memories/:id/retire", post(v1::retire_memory))
        .route("/v1/memories/:id", delete(v1::delete_memory))
        // The four graph reads (ADR-021); the person actions have no route (SA-GRAPH-09).
        .route("/v1/memories/:id/history", get(v1::fact_history))
        .route("/v1/memories/:id/origins", get(v1::fact_origins))
        .route("/v1/entities", get(v1::list_entities))
        .route("/v1/entities/:id", get(v1::entity_detail))
        .route("/openapi.json", get(v1::openapi_doc))
        // --- operational routes (bypass auth/rate-limit/idempotency/audit) ---
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/metrics", get(health::metrics))
        .fallback(not_found_fallback)
        // Body-size limit (chain step 2) — applied before correlation-id so correlation-id wraps it.
        .layer(DefaultBodyLimit::max(max_body))
        // Correlation-id assignment (chain step 1, outermost).
        .layer(middleware::from_fn(correlation_id_middleware))
        .layer(envelope::panic_recovery_layer())
        .with_state(state)
}

/// Assign or propagate the correlation id (chain step 1). A valid inbound `x-correlation-id` is
/// honoured; otherwise a fresh UUID v4 is minted. The id is stored in request extensions for handlers
/// and echoed on the response header so a caller can correlate logs/traces.
async fn correlation_id_middleware(mut req: Request, next: Next) -> Response {
    let incoming = req
        .headers()
        .get(CORRELATION_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| Uuid::parse_str(s).is_ok())
        .map(|s| s.to_string());
    let correlation_id = incoming.unwrap_or_else(|| Uuid::new_v4().to_string());

    req.extensions_mut()
        .insert(CorrelationId(correlation_id.clone()));

    let mut resp = next.run(req).await;
    if let Ok(value) = HeaderValue::from_str(&correlation_id) {
        resp.headers_mut().insert(CORRELATION_ID_HEADER, value);
    }
    resp
}

/// Per-request correlation id, stored as a request extension by the middleware.
#[derive(Clone)]
pub struct CorrelationId(pub String);

/// Read the correlation id from request extensions, minting a fresh one if absent (e.g. a request that
/// somehow bypassed the middleware).
pub(crate) fn correlation_from(req: &Request, state: &AppState) -> String {
    req.extensions()
        .get::<CorrelationId>()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| state.new_correlation_id())
}

/// Render an [`AppError`] into the X1 error envelope response, carrying the correlation id in both the
/// body and the `X-Correlation-Id` header. The single conversion the edge uses for every error path.
pub(crate) fn error_response(err: &AppError, correlation_id: &str, env: Env) -> Response {
    let (status, mut envelope) = crate::error::map_error(err, correlation_id, env);
    // Guarantee the correlation id is the request id even if map_error was given a blank one.
    envelope.error.correlation_id = correlation_id.to_string();
    let mut resp = (status, Json(envelope)).into_response();
    if let Ok(value) = HeaderValue::from_str(correlation_id) {
        resp.headers_mut().insert(CORRELATION_ID_HEADER, value);
    }
    resp
}

/// Fallback for an unmatched route: the X1 `NOT_FOUND` (404) error envelope with the request's
/// correlation id.
async fn not_found_fallback(State(state): State<AppState>, req: Request) -> Response {
    let correlation_id = correlation_from(&req, &state);
    let (status, envelope) =
        crate::error::map_error(&AppError::NotFound, &correlation_id, state.env());
    debug_assert_eq!(status, StatusCode::NOT_FOUND);
    let envelope = ErrorEnvelope {
        error: ErrorBody {
            code: envelope.error.code,
            message: envelope.error.message,
            correlation_id,
        },
    };
    (status, Json(envelope)).into_response()
}
