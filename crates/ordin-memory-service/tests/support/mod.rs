//! What the integration test binaries share.
#![allow(dead_code)]

pub mod models;
pub mod pg;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use models::{WordEmbedder, WordReranker};
use ordin_memory_service::{MemoryServiceConfig, ModelManifest, ProgramKind, TestSeams};

pub fn config(program: ProgramKind) -> MemoryServiceConfig {
    MemoryServiceConfig {
        program,
        models_dir: PathBuf::from("/nonexistent/ordin-models"),
        agent_wide_scope: None,
        log_level: tracing::Level::INFO,
        recall_log_days: 90,
        knowledge_scope: None,
        model_manifest: ModelManifest::default(),
    }
}

pub fn seams() -> TestSeams {
    TestSeams {
        embedder: Arc::new(WordEmbedder),
        reranker: Arc::new(WordReranker),
        rate_per_min: 60_000_000,
        reconcile_every: Duration::from_millis(20),
        recall_limit_secs: 10,
    }
}
