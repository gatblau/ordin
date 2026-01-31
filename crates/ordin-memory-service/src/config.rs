//! The configuration of this crate and the memory component's `Config` built in Rust
//! (*Shared Context* §5). No `MEMORY_*` variable is read or set (2D-1e).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use memory::config::{Config, LogLevel, QueueBackend, Secret, StoreBackend};
use memory::error::Env;
use memory::types::placement::MemoryProfile;
use ordin_types::ScopeId;

/// The tenant of every memory row.
pub(crate) const TENANT: &str = "ordin";
/// The `client` every in-process context carries.
pub(crate) const IN_PROCESS_CLIENT: &str = "ordin-in-process";
/// `intfloat/multilingual-e5-small` (SA-MEM-01).
pub(crate) const EMBED_DIM: u32 = 384;
/// Queue lease reaper period (`queue_reaper_secs`).
pub(crate) const REAPER_SECS: u32 = 30;
/// Reconciler period.
pub(crate) const RECONCILE_SECS: u64 = 2;
/// The overall time limit of one recall (memory SA-COLL-06), in-process.
pub(crate) const RECALL_LIMIT_SECS: u32 = 10;
/// Retention sweep period.
pub(crate) const SWEEP_SECS: u64 = 60 * 60;
/// Model idle unload.
pub(crate) const MODEL_IDLE_SECS: u64 = 10 * 60;

/// How the program runs; a provider that must tell the single and scaled shapes apart reads this
/// (reconciliation row 285).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramKind {
    Desktop,
    PersonalServer,
    AgentSingle,
    AgentScaled,
}

impl ProgramKind {
    pub(crate) fn is_agent_profile(self) -> bool {
        matches!(self, ProgramKind::AgentSingle | ProgramKind::AgentScaled)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ProgramKind::Desktop => "desktop",
            ProgramKind::PersonalServer => "personal_server",
            ProgramKind::AgentSingle => "agent_single",
            ProgramKind::AgentScaled => "agent_scaled",
        }
    }
}

/// One bundled file and its SHA-256 (lowercase hex).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFileDigest {
    /// Relative to `models_dir`, for example `embedding/model.onnx`.
    pub path: String,
    pub sha256: String,
}

/// The build-time list of bundled model files and their digests (*Internal Logic* §1 step 3).
// The manifest is an input of `MemoryServiceConfig`, the one place a program can pass it, because the
// bundling belongs to the program steps.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelManifest {
    pub files: Vec<ModelFileDigest>,
}

impl ModelManifest {
    pub(crate) fn digest_of(&self, path: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|f| f.path == path)
            .map(|f| f.sha256.as_str())
    }
}

pub struct MemoryServiceConfig {
    pub program: ProgramKind,
    /// Directory holding the two ONNX models and their tokenizer files.
    pub models_dir: PathBuf,
    /// Agent profile only: the agent-wide scope (`ScopeKind::AgentWide`); None elsewhere.
    pub agent_wide_scope: Option<ScopeId>,
    pub log_level: tracing::Level,
    /// SA-AMEM-10: days the recall log keeps its rows; 1–3,650, default 90.
    pub recall_log_days: u16,
    /// Agent profile whose configuration has a `knowledge-agent` section: the scope of kind
    /// `knowledge`; None elsewhere.
    pub knowledge_scope: Option<ScopeId>,
    /// The digests of the bundled files.
    pub model_manifest: ModelManifest,
}

fn log_level(level: tracing::Level) -> LogLevel {
    match level {
        tracing::Level::ERROR => LogLevel::Error,
        tracing::Level::WARN => LogLevel::Warn,
        tracing::Level::INFO => LogLevel::Info,
        tracing::Level::DEBUG => LogLevel::Debug,
        tracing::Level::TRACE => LogLevel::Trace,
    }
}

/// `memory::config::Config` per *Shared Context* §5, with the crate defaults of `Config::load` for
/// every field the table does not change.
pub(crate) fn component_config(
    level: tracing::Level,
    embed_model_version: String,
    rate_per_min: u32,
) -> Config {
    let unbound = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    Config {
        http_addr: unbound,
        mcp_http_addr: unbound,
        mcp_path: "/mcp".to_owned(),
        store_path: String::new(),
        store_remote_url: None,
        store_remote_user: None,
        store_remote_pass: None,
        store_backend: StoreBackend::SurrealKv,
        oidc_issuer: String::new(),
        oidc_audience: String::new(),
        oidc_subject_claim: "sub".to_owned(),
        oidc_teams_claim: "groups".to_owned(),
        oidc_tenant_claim: "tenant".to_owned(),
        oidc_client_claim: "azp".to_owned(),
        oidc_authority_claim: "memory_fact_authority".to_owned(),
        jwks_refresh_secs: 3600,
        embed_url: String::new(),
        embed_api_key: Secret::new(String::new()),
        embed_dim: EMBED_DIM,
        memory_profile: MemoryProfile::Collective,
        rerank_url: String::new(),
        rerank_api_key: Secret::new(String::new()),
        queue_backend: QueueBackend::Store,
        queue_nats_url: None,
        job_max_attempts: 5,
        job_backoff_base_ms: 2000,
        queue_reaper_secs: REAPER_SECS,
        queue_poll_ms: 500,
        dead_letter_ttl_secs: 2_592_000,
        queue_max_open: 5_000,
        done_job_ttl_secs: 604_800,
        recall_time_limit_secs: RECALL_LIMIT_SECS,
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
        // spec: ordin-desktop/components/MemoryService#SA-AMEM-03
        decay_k: 864_000.0,
        prune_retrievability: 0.05,
        idle_quiet_secs: 300,
        maint_max_interval_secs: 21600,
        maint_batch_size: 500,
        reinforce_gain: 0.5,
        idempotency_ttl_secs: 86400,
        rate_read_per_min: rate_per_min,
        rate_write_per_min: rate_per_min,
        max_body_bytes: 1_048_576,
        embed_model_version,
        otlp_endpoint: None,
        log_level: log_level(level),
        env: Env::Production,
    }
}
