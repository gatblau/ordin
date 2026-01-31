//! C8 — HTTP API Edge. The single externally-reachable surface of `memory`.
//!
//! The edge terminates HTTP, exposes the task-shaped `/v1` routes (capabilities, recall, remember,
//! get-fact, retire, delete, OpenAPI) and the three unauthenticated operational routes (`/healthz`,
//! `/readyz`, `/metrics`). It owns the per-request middleware chain and the two response contracts —
//! the single `Success<T>` envelope and the single `ErrorEnvelope` — with every component error mapped
//! to a registered `(HTTP status, machine code)` pair by `crate::error::map_error`. It holds no domain
//! logic: recall is delegated to C6, writes are enqueued on C2, fact reads + retire go through C1, and
//! the verifiable hard delete runs against C1 directly (see `v1::delete_memory`).
//!
//! ## Middleware chain order (binding, outermost-first)
//!
//! `correlation-id → body-size limit → auth (C3) → rate limit → idempotency (writes only) → handler
//! → audit`. The two outermost concerns (correlation-id, body-size limit) are tower layers on the
//! router so their order is structural. The inner concerns (auth → rate limit → idempotency → handler
//! → audit) run inside each `/v1` handler via [`EdgePipeline`], which executes them in exactly that
//! fixed order; the operational routes bypass auth/rate-limit/idempotency/audit entirely.
//!
//! The edge is built only with features `http` and `surreal` (it serves the standalone SurrealDB
//! deployment); `ratelimit`, which the transport-neutral Service also uses, is always built.

#[cfg(all(feature = "http", feature = "surreal"))]
mod edge;
pub mod ratelimit;

#[cfg(all(feature = "http", feature = "surreal"))]
pub use edge::*;
