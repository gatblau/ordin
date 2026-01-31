//! `memory` — agentic memory service.
//!
//! Phase 1 (scaffolding & cross-cutting foundations): shared types (§2C), the X1 error registry,
//! X6 configuration, X3/X4/X5 observability seams, skeleton provider adapters, and a minimal bootable
//! axum server with the operational endpoints. Component logic (C1–C8) lands in later phases.
//!
//! Cargo features (ADR-101): `surreal` — the SurrealDB storage (`store::surreal`) and the benchmark
//! harness; `http` — the HTTP and MCP edges and the OIDC `Authenticator`; both are on by default and
//! the standalone binaries require them. `embedded` — the in-process constructor
//! (`embedded::build`) for a host that supplies its own storage, providers and caller
//! authenticator. `sql` — the SQLite and PostgreSQL storage (`store::sql`).

pub mod api;
pub mod auth;
#[cfg(feature = "surreal")]
pub mod bench;
pub mod config;
#[cfg(feature = "embedded")]
pub mod embedded;
pub mod error;
pub mod maintenance;
#[cfg(feature = "http")]
pub mod mcp;
pub mod obs;
pub mod providers;
pub mod queue;
pub mod retrieval;
pub mod service;
#[cfg(feature = "http")]
pub mod shutdown;
pub mod store;
pub mod text;
pub mod types;
pub mod write_pipeline;

#[cfg(all(feature = "http", feature = "surreal"))]
mod standalone {
    use std::collections::HashMap;
    use std::sync::Arc;

    use anyhow::Context;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    use crate::api::{build_router, AppState};
    use crate::auth::{AuthConfig, Authenticator};
    use crate::config::Config;
    use crate::obs::metrics::Metrics;
    use crate::providers::{HttpEmbeddingClient, HttpRerankClient};
    use crate::retrieval::{RetrievalConfig, RetrievalEngine};
    use crate::store::surreal::Store;
    use crate::types::ports::{EmbeddingClient, RerankClient, StoragePort};
    use crate::{obs, shutdown};

    /// Build the full production application state from a loaded configuration: the embedded store (C1), the
    /// store-backed work queue (C2), the provider HTTP clients, the retrieval engine (C6), and the
    /// authenticator (C3, which performs OIDC discovery + the first JWKS fetch and so fails fast if the IdP
    /// is unreachable). Registers the metric catalogue and initialises the optional tracing export seam.
    /// Freshness is agent-side (ADR-014): recall holds no broker client and runs no freshness checker.
    ///
    /// DEVIATION (documented follow-up): the embedded store is opened with `Store::connect`, which uses the
    /// SurrealKV embedded engine at `MEMORY_STORE_PATH`. No production "open" constructor beyond
    /// `connect`/`new_in_memory` exists, so `connect` is used as-is.
    pub async fn build_state(config: Config) -> anyhow::Result<AppState> {
        let config = Arc::new(config);
        obs::trace::init_tracing(&config);

        // C1 — embedded store, carrying the store-backed C2 work queue over its own connection.
        let store = Arc::new(
            Store::connect(&config)
                .await
                .context("opening the embedded store")?,
        );

        // Providers (C4/C6 seams).
        let embedder: Arc<dyn EmbeddingClient> = Arc::new(HttpEmbeddingClient::new(&config));
        let reranker: Arc<dyn RerankClient> = Arc::new(HttpRerankClient::new(&config));

        // C6 — retrieval engine. `store.clone()` coerces `Arc<Store>` to `Arc<dyn StoragePort>`.
        // Freshness is agent-side (ADR-014): memory makes no broker call and runs no freshness checker.
        let store_dyn: Arc<dyn StoragePort> = store.clone();
        let engine = Arc::new(RetrievalEngine::new(
            store_dyn,
            embedder,
            reranker,
            RetrievalConfig::from_config(&config),
        ));

        // C3 — authenticator (OIDC discovery + first JWKS fetch; fails fast).
        let auth = Arc::new(
            Authenticator::new(AuthConfig::from_config(&config))
                .await
                .context("constructing the authenticator")?,
        );

        Ok(AppState {
            config,
            metrics: Metrics::new(),
            store,
            engine,
            auth,
            rate: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Bind the configured HTTP address and serve until a shutdown signal is received.
    ///
    /// Returns the bound [`AppState`] error context on failure. Used by `main`; the BDD harness uses
    /// [`serve_on_listener`] with an ephemeral port instead.
    pub async fn serve(state: AppState) -> anyhow::Result<()> {
        let addr = state.config.http_addr;
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding HTTP address {addr}"))?;
        serve_on_listener(listener, state).await
    }

    /// Serve the router on an already-bound listener until a shutdown signal is received. Exposed so the
    /// integration harness can bind an ephemeral port and drive the real app in-process.
    pub async fn serve_on_listener(listener: TcpListener, state: AppState) -> anyhow::Result<()> {
        let router = build_router(state);
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown::shutdown_signal())
            .await
            .context("serving HTTP")?;
        Ok(())
    }
}

#[cfg(all(feature = "http", feature = "surreal"))]
pub use standalone::{build_state, serve, serve_on_listener};
