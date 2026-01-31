//! The embedded constructor (feature `embedded`): the component stack assembled inside a host
//! process, with no HTTP edge, no OIDC and no environment configuration.
//!
//! The standalone binaries assemble the stack through `build_state`, which loads nothing itself but
//! opens the SurrealDB store, builds the HTTP provider clients, performs OIDC discovery and starts the
//! tracing seam. An embedding host has already made each of those choices: it hands over a built
//! [`Config`], the storage, the three providers and the caller authenticator as trait objects, and
//! receives the Service, the write pipeline, the maintenance worker and the storage back. [`build`]
//! performs no I/O, reads no environment variable, installs no global subscriber and spawns no task;
//! the host spawns `WritePipeline::run`, `MaintenanceWorker::run_scheduler`,
//! `MaintenanceWorker::run_consumer` and a `LeaseReaper` under its own shutdown token.

use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::CallerAuthenticator;
use crate::config::Config;
use crate::maintenance::{MaintenanceConfig, MaintenanceWorker};
use crate::obs::metrics::Metrics;
use crate::retrieval::{RetrievalConfig, RetrievalEngine};
use crate::service::Service;
use crate::types::ports::{EmbeddingClient, PiiDetector, RerankClient, StoragePort};
use crate::write_pipeline::{WritePipeline, WritePipelineConfig};

/// The injected seams of an embedded stack.
pub struct EmbeddedDeps {
    pub storage: Arc<dyn StoragePort>,
    pub embedder: Arc<dyn EmbeddingClient>,
    pub reranker: Arc<dyn RerankClient>,
    pub pii: Arc<dyn PiiDetector>,
    pub authenticator: Arc<dyn CallerAuthenticator>,
}

/// The assembled stack. Every handle shares the one `storage`.
pub struct EmbeddedMemory {
    pub service: Arc<Service>,
    pub write_pipeline: Arc<WritePipeline>,
    pub maintenance: Arc<MaintenanceWorker>,
    pub storage: Arc<dyn StoragePort>,
}

/// Assemble the component stack over the injected seams. `config` is taken as given: this never calls
/// `Config::load`, `build_state` or `obs::log::init_logging`.
pub fn build(config: Arc<Config>, deps: EmbeddedDeps) -> EmbeddedMemory {
    let EmbeddedDeps {
        storage,
        embedder,
        reranker,
        pii,
        authenticator,
    } = deps;

    // C6 — the read path.
    let engine = Arc::new(RetrievalEngine::new(
        storage.clone(),
        embedder.clone(),
        reranker,
        RetrievalConfig::from_config(&config),
    ));

    // C4 — the asynchronous write consumer.
    let write_pipeline = Arc::new(WritePipeline::new(
        storage.clone(),
        embedder.clone(),
        pii,
        WritePipelineConfig::from_config(&config),
    ));

    // C9 — the orchestration core, with its own rate-limiter map (no edge shares it here), and the
    // write pipeline whose gate `import_facts` runs (ordin change M-7 b).
    let service = Arc::new(
        Service::new(
            config.clone(),
            Metrics::new(),
            storage.clone(),
            engine,
            authenticator,
            Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        )
        .with_write_pipeline(write_pipeline.clone()),
    );

    // C7 — the background maintenance worker.
    let maintenance = Arc::new(MaintenanceWorker::new(
        storage.clone(),
        embedder,
        MaintenanceConfig::from_config(&config),
    ));

    EmbeddedMemory {
        service,
        write_pipeline,
        maintenance,
        storage,
    }
}

#[cfg(test)]
mod tests {
    //! `build` is exercised end to end over the in-memory SurrealDB engine (the only storage this
    //! crate carries until the SQL storage lands), with in-process stand-ins for every provider and an
    //! authenticator that maps a fixed bearer to a fixed context.

    /// The code of this file above the test module — comments aside, which name these entry points
    /// only to say they are not called — reaches none of the standalone bootstrap entry points and no
    /// environment variable. The needles are assembled at run time so this test's own text cannot
    /// match them.
    #[test]
    fn build_names_no_standalone_bootstrap() {
        let src = include_str!("embedded.rs");
        let production: String = src
            .split(&["#[cfg(", "test)]"].concat())
            .next()
            .expect("source")
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            ["Config::", "load"].concat(),
            ["build_", "state"].concat(),
            ["init_", "logging"].concat(),
            ["std::", "env"].concat(),
        ] {
            assert!(
                !production.contains(&needle),
                "embedded::build must not reach `{needle}`"
            );
        }
    }

    #[cfg(feature = "surreal")]
    mod round_trip {
        use std::net::SocketAddr;
        use std::time::Duration;

        use async_trait::async_trait;
        use tokio_util::sync::CancellationToken;

        use super::super::*;
        use crate::auth::AuthError;
        use crate::config::{LogLevel, QueueBackend, Secret, StoreBackend};
        use crate::error::Env;
        use crate::providers::LocalPiiDetector;
        use crate::service::CallContext;
        use crate::store::surreal::Store;
        use crate::types::api::JobAckStatus;
        use crate::types::placement::MemoryProfile;
        use crate::types::ports::ProviderError;
        use crate::types::scope::{OpSet, ScopeContext};

        const DIM: u32 = 8;
        const BEARER: &str = "in-process-bearer";

        /// Every vector is the same unit vector, so every stored fact is a perfect vector match.
        struct UnitEmbedder;

        #[async_trait]
        impl EmbeddingClient for UnitEmbedder {
            async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
                let mut v = vec![0.0f32; DIM as usize];
                v[0] = 1.0;
                Ok(texts.iter().map(|_| v.clone()).collect())
            }
        }

        /// Scores every document 0.9, comfortably above the abstain threshold.
        struct UniformReranker;

        #[async_trait]
        impl RerankClient for UniformReranker {
            async fn rerank(
                &self,
                _query: &str,
                docs: &[String],
            ) -> Result<Vec<f64>, ProviderError> {
                Ok(vec![0.9; docs.len()])
            }
        }

        /// Maps exactly one bearer to one context, with the caller's correlation id threaded in.
        struct FixedAuthenticator;

        #[async_trait]
        impl CallerAuthenticator for FixedAuthenticator {
            async fn validate(
                &self,
                bearer: &str,
                correlation_id: &str,
            ) -> Result<ScopeContext, AuthError> {
                if bearer.is_empty() {
                    return Err(AuthError::MissingToken);
                }
                if bearer != BEARER {
                    return Err(AuthError::InvalidToken("unknown in-process bearer".into()));
                }
                Ok(ScopeContext {
                    tenant: "ordin".into(),
                    teams: vec![],
                    user: "person-1".into(),
                    token_jti: BEARER[..8].to_string(),
                    allowed_ops: OpSet {
                        read: true,
                        write: true,
                        forget: true,
                    },
                    client: Some("ordin-in-process".into()),
                    fact_authority: None,
                    correlation_id: correlation_id.to_string(),
                })
            }
        }

        /// A configuration built in Rust, as a host builds it. The OIDC and provider fields are
        /// empty, which `Config::load` would refuse — so a stack that serves under it was not
        /// configured from the environment.
        fn host_config() -> Config {
            let unbound: SocketAddr = "127.0.0.1:0".parse().expect("addr");
            Config {
                http_addr: unbound,
                mcp_http_addr: unbound,
                mcp_path: "/mcp".into(),
                store_path: String::new(),
                store_remote_url: None,
                store_remote_user: None,
                store_remote_pass: None,
                store_backend: StoreBackend::SurrealKv,
                oidc_issuer: String::new(),
                oidc_audience: String::new(),
                oidc_subject_claim: "sub".into(),
                oidc_teams_claim: "groups".into(),
                oidc_tenant_claim: "tenant".into(),
                oidc_client_claim: "azp".into(),
                oidc_authority_claim: "memory_fact_authority".into(),
                jwks_refresh_secs: 3600,
                embed_url: String::new(),
                embed_api_key: Secret::new(String::new()),
                embed_dim: DIM,
                memory_profile: MemoryProfile::Collective,
                rerank_url: String::new(),
                rerank_api_key: Secret::new(String::new()),
                queue_backend: QueueBackend::Store,
                queue_nats_url: None,
                job_max_attempts: 5,
                job_backoff_base_ms: 2000,
                queue_reaper_secs: 30,
                queue_poll_ms: 500,
                dead_letter_ttl_secs: 2_592_000,
                queue_max_open: 5_000,
                done_job_ttl_secs: 604_800,
                recall_time_limit_secs: 10,
                result_cap_max: 50,
                stage1_k: 50,
                abstain_threshold: 0.2,
                recency_weight: 0.15,
                recency_tau_days: 30.0,
                reformulation_enabled: false,
                graph_hops: 1,
                graph_fanout: 32,
                graph_cap: 200,
                trust_weight: 0.1,
                passage_class: "passage".into(),
                trust_admit: 0.7,
                trust_quarantine: 0.4,
                pii_redact_conf: 0.9,
                source_trust_default: 0.5,
                salience_floor: 0.3,
                decay_k: 10.0,
                prune_retrievability: 0.05,
                idle_quiet_secs: 300,
                maint_max_interval_secs: 21600,
                maint_batch_size: 500,
                reinforce_gain: 0.5,
                idempotency_ttl_secs: 86400,
                rate_read_per_min: 600,
                rate_write_per_min: 600,
                max_body_bytes: 1_048_576,
                embed_model_version: "test@000000000000".into(),
                otlp_endpoint: None,
                log_level: LogLevel::Info,
                env: Env::Development,
            }
        }

        async fn embedded() -> EmbeddedMemory {
            let storage: Arc<dyn StoragePort> =
                Arc::new(Store::new_in_memory(DIM).await.expect("in-memory store"));
            build(
                Arc::new(host_config()),
                EmbeddedDeps {
                    storage,
                    embedder: Arc::new(UnitEmbedder),
                    reranker: Arc::new(UniformReranker),
                    pii: Arc::new(LocalPiiDetector::new()),
                    authenticator: Arc::new(FixedAuthenticator),
                },
            )
        }

        #[tokio::test]
        async fn remember_then_recall_round_trips_in_process() {
            let mem = embedded().await;

            let ack = mem
                .service
                .remember(
                    CallContext {
                        bearer: BEARER,
                        correlation_id: "corr-remember",
                        idempotency_key: Some("embedded-rt-1"),
                    },
                    br#"{"content":{"subject":"ordin","predicate":"stores","object":"memories"}}"#,
                )
                .await
                .unwrap_or_else(|e| panic!("remember failed: {}", e.error.code()));
            assert!(matches!(ack.data.status, JobAckStatus::Accepted));

            // The host runs the write consumer; here it runs until the fact is recallable.
            let cancel = CancellationToken::new();
            let pipeline = mem.write_pipeline.clone();
            let consumer = tokio::spawn({
                let cancel = cancel.clone();
                async move { pipeline.run(cancel).await }
            });

            let mut found = None;
            for _ in 0..100 {
                let out = mem
                    .service
                    .recall(
                        CallContext {
                            bearer: BEARER,
                            correlation_id: "corr-recall",
                            idempotency_key: None,
                        },
                        br#"{"query":"what does ordin store","result_cap":5}"#,
                    )
                    .await
                    .unwrap_or_else(|e| panic!("recall failed: {}", e.error.code()));
                if let Some(rf) = out.data.response.facts.into_iter().next() {
                    found = Some(rf.fact);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            cancel.cancel();
            consumer.await.expect("consumer task");

            let fact = found.expect("the remembered fact is recalled");
            assert_eq!(fact.content["subject"], "ordin");
            assert_eq!(fact.owner.tenant, "ordin");
            assert_eq!(fact.owner.user, "person-1");
        }

        /// The registered authenticator is the one the Service consults: a bearer it does not know
        /// is refused as an invalid token before any storage is touched.
        #[tokio::test]
        async fn an_unknown_bearer_is_refused_by_the_injected_authenticator() {
            let mem = embedded().await;
            let err = match mem
                .service
                .capabilities(CallContext {
                    bearer: "someone-else",
                    correlation_id: "corr-cap",
                    idempotency_key: None,
                })
                .await
            {
                Ok(_) => panic!("an unknown bearer must be refused"),
                Err(e) => e,
            };
            assert_eq!(err.error.code(), "AUTH_INVALID_TOKEN");
        }
    }
}
