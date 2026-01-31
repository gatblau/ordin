//! `OnnxEmbedder` and `OnnxReranker`: the two bundled models in-process through `fastembed`
//! (ADR-033, SA-MEM-01), loaded on first use and unloaded after 10 minutes without a call.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fastembed::{
    InitOptionsUserDefined, Pooling, RerankInitOptionsUserDefined, TextEmbedding, TextRerank,
    TokenizerFiles, UserDefinedEmbeddingModel, UserDefinedRerankingModel,
};
use memory::types::ports::{EmbeddingClient, ProviderError, RerankClient};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::{ModelManifest, MODEL_IDLE_SECS};

/// The bundled files, relative to `MemoryServiceConfig::models_dir`.
pub(crate) const EMBEDDING_MODEL: &str = "embedding/model.onnx";
pub(crate) const RERANK_MODEL: &str = "reranker/model.onnx";
const TOKENIZER_FILES: [&str; 4] = [
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];
/// `intfloat/multilingual-e5-small` and the reranker both read at most 512 tokens.
const MAX_LENGTH: usize = 512;

/// Every file §1 step 3 checks.
pub(crate) fn bundled_files() -> Vec<String> {
    let mut files = vec![EMBEDDING_MODEL.to_owned(), RERANK_MODEL.to_owned()];
    for model in ["embedding", "reranker"] {
        for name in TOKENIZER_FILES {
            files.push(format!("{model}/{name}"));
        }
    }
    files
}

/// §1 step 3: presence and SHA-256 of every bundled file against the manifest. Logs
/// `memory.model_missing { file }` for each that fails; the service still starts.
pub(crate) async fn check_models(models_dir: &Path, manifest: &ModelManifest) -> bool {
    let dir = models_dir.to_path_buf();
    let manifest = manifest.clone();
    let failed = tokio::task::spawn_blocking(move || {
        bundled_files()
            .into_iter()
            .filter(|file| {
                let digest = crate::service::file_sha256(&dir.join(file)).ok();
                digest.is_none() || digest.as_deref() != manifest.digest_of(file)
            })
            .collect::<Vec<String>>()
    })
    .await
    .unwrap_or_else(|_| bundled_files());
    // MEM-MD-6: when no model file is there at all, the models are not downloaded yet (the person has not
    // agreed, or the build has no models input): one info line, not ten errors Diagnostics would show.
    // A partial or damaged set is still an error per file.
    // spec: ordin-desktop/components/MemoryService#MEM-MD-6
    let none_present = !failed.is_empty() && failed.len() == bundled_files().len() && {
        let dir = models_dir.to_path_buf();
        tokio::task::spawn_blocking(move || bundled_files().iter().all(|f| !dir.join(f).exists()))
            .await
            .unwrap_or(false)
    };
    if none_present {
        tracing::info!(target: "ordin::memory", files = failed.len(), "memory.models_not_downloaded");
    } else {
        for file in &failed {
            tracing::error!(target: "ordin::memory", file = %file, "memory.model_missing");
        }
    }
    failed.is_empty()
}

tokio::task_local! {
    static RECALL_QUERY: ();
}

/// Runs `f` with the embedder reading its text as a recall query.
// `EmbeddingClient::embed` is one seam for the three components that embed, and the model needs to
// know which text it has (*Shared Context* §6): "query: " for a recall, "passage: " for stored
// content. A recall embeds inside `Service::recall`, on the caller's task, so a task-local marks it.
pub(crate) async fn as_recall_query<F: Future>(f: F) -> F::Output {
    RECALL_QUERY.scope((), f).await
}

fn input_prefix() -> &'static str {
    if RECALL_QUERY.try_with(|_| ()).is_ok() {
        "query: "
    } else {
        "passage: "
    }
}

/// ONNX Runtime allows one environment per process, and `fastembed` builds its own for a model given
/// none, so the second model would fail to load. Both are given this one.
fn onnx_environment() -> Result<ort::environment::Environment, ProviderError> {
    static ENVIRONMENT: OnceLock<ort::environment::Environment> = OnceLock::new();
    if let Some(environment) = ENVIRONMENT.get() {
        return Ok(environment.clone());
    }
    let built = ort::init()
        .with_name("ordin-memory")
        .build()
        .map_err(load_failed)?;
    Ok(ENVIRONMENT.get_or_init(|| built).clone())
}

struct Slot<T> {
    model: Option<T>,
    last_used: Instant,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot {
            model: None,
            last_used: Instant::now(),
        }
    }
}

/// The two models of one service.
pub(crate) struct Models {
    dir: PathBuf,
    manifest: ModelManifest,
    embedding: Arc<Mutex<Slot<TextEmbedding>>>,
    reranker: Arc<Mutex<Slot<TextRerank>>>,
}

/// A bundled file's bytes, once they match the manifest. These are the bytes the parser is given:
/// a file changed after the check at start, or between the check and the read, is refused here.
fn read_checked(
    dir: &Path,
    manifest: &ModelManifest,
    file: &str,
) -> Result<Vec<u8>, ProviderError> {
    let bytes = std::fs::read(dir.join(file)).map_err(load_failed)?;
    let digest = ordin_types::num::Sha256Hex::from_digest(&Sha256::digest(&bytes).into()).0;
    if manifest.digest_of(file) != Some(digest.as_str()) {
        tracing::error!(target: "ordin::memory", file = %file, "memory.model_missing");
        return Err(ProviderError::Transport(
            "the model could not be loaded".to_owned(),
        ));
    }
    Ok(bytes)
}

fn read_tokenizer(
    dir: &Path,
    manifest: &ModelManifest,
    model: &str,
) -> Result<TokenizerFiles, ProviderError> {
    let read = |name: &str| read_checked(dir, manifest, &format!("{model}/{name}"));
    Ok(TokenizerFiles {
        tokenizer_file: read("tokenizer.json")?,
        config_file: read("config.json")?,
        special_tokens_map_file: read("special_tokens_map.json")?,
        tokenizer_config_file: read("tokenizer_config.json")?,
    })
}

fn load_failed(error: impl std::fmt::Display) -> ProviderError {
    // A loader's error names a file or an operator, never fact content.
    tracing::error!(target: "ordin::memory", detail = %error, "memory.model_load_failed");
    ProviderError::Transport("the model could not be loaded".to_owned())
}

fn load_embedding(dir: &Path, manifest: &ModelManifest) -> Result<TextEmbedding, ProviderError> {
    let started = Instant::now();
    let tokenizer = read_tokenizer(dir, manifest, "embedding")?;
    let onnx = read_checked(dir, manifest, EMBEDDING_MODEL)?;
    let model = TextEmbedding::try_new_from_user_defined(
        UserDefinedEmbeddingModel::new(onnx, tokenizer).with_pooling(Pooling::Mean),
        InitOptionsUserDefined::new()
            .with_environment(onnx_environment()?)
            .with_max_length(MAX_LENGTH),
    )
    .map_err(load_failed)?;
    tracing::info!(
        target: "ordin::memory",
        model = "embedding",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "memory.model_loaded"
    );
    Ok(model)
}

fn load_reranker(dir: &Path, manifest: &ModelManifest) -> Result<TextRerank, ProviderError> {
    let started = Instant::now();
    let tokenizer = read_tokenizer(dir, manifest, "reranker")?;
    let onnx = read_checked(dir, manifest, RERANK_MODEL)?;
    let model = TextRerank::try_new_from_user_defined(
        UserDefinedRerankingModel::new(onnx, tokenizer),
        RerankInitOptionsUserDefined::new()
            .with_environment(onnx_environment()?)
            .with_max_length(MAX_LENGTH),
    )
    .map_err(load_failed)?;
    tracing::info!(
        target: "ordin::memory",
        model = "reranker",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "memory.model_loaded"
    );
    Ok(model)
}

/// The cross-encoder reads prose: a stored fact's content is its statement and subject.
fn plain_text(document: &str) -> String {
    match serde_json::from_str::<Value>(document) {
        Ok(Value::Object(content)) => {
            let text = |key: &str| content.get(key).and_then(Value::as_str);
            match (text("subject"), text("statement")) {
                (Some(subject), Some(statement)) => format!("{subject}. {statement}"),
                (None, Some(statement)) => statement.to_owned(),
                _ => document.to_owned(),
            }
        }
        _ => document.to_owned(),
    }
}

/// A cross-encoder answers a logit; the component's final score reads a probability in [0, 1].
fn probability(logit: f32) -> f64 {
    1.0 / (1.0 + f64::from(-logit).exp())
}

impl Models {
    pub(crate) fn new(dir: PathBuf, manifest: ModelManifest) -> Models {
        Models {
            dir,
            manifest,
            embedding: Arc::default(),
            reranker: Arc::default(),
        }
    }

    /// The folder and manifest of §1 step 3, for `recheck_models` (MEM-MD-3).
    pub(crate) fn source(&self) -> (&Path, &ModelManifest) {
        (&self.dir, &self.manifest)
    }

    pub(crate) fn embedder(self: &Arc<Self>) -> Arc<OnnxEmbedder> {
        Arc::new(OnnxEmbedder {
            models: self.clone(),
        })
    }

    pub(crate) fn reranker(self: &Arc<Self>) -> Arc<OnnxReranker> {
        Arc::new(OnnxReranker {
            models: self.clone(),
        })
    }

    /// Drops a model unused for 10 minutes (`force`: at shutdown).
    pub(crate) fn unload(&self, force: bool) {
        let idle = Duration::from_secs(MODEL_IDLE_SECS);
        if let Ok(mut slot) = self.embedding.lock() {
            if slot.model.is_some() && (force || slot.last_used.elapsed() >= idle) {
                slot.model = None;
                tracing::info!(target: "ordin::memory", model = "embedding", "memory.model_unloaded");
            }
        }
        if let Ok(mut slot) = self.reranker.lock() {
            if slot.model.is_some() && (force || slot.last_used.elapsed() >= idle) {
                slot.model = None;
                tracing::info!(target: "ordin::memory", model = "reranker", "memory.model_unloaded");
            }
        }
    }
}

pub(crate) struct OnnxEmbedder {
    models: Arc<Models>,
}

#[async_trait]
impl EmbeddingClient for OnnxEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        let prefix = input_prefix();
        let inputs: Vec<String> = texts.iter().map(|t| format!("{prefix}{t}")).collect();
        let models = self.models.clone();
        tokio::task::spawn_blocking(move || {
            let mut slot = models
                .embedding
                .lock()
                .map_err(|_| ProviderError::Transport("the embedder failed earlier".to_owned()))?;
            slot.last_used = Instant::now();
            if slot.model.is_none() {
                slot.model = Some(load_embedding(&models.dir, &models.manifest)?);
            }
            let model = slot
                .model
                .as_mut()
                .ok_or_else(|| ProviderError::Transport("no embedder".to_owned()))?;
            model
                .embed(inputs, None)
                .map_err(|_| ProviderError::Malformed("the embedder failed".to_owned()))
        })
        .await
        .map_err(|_| ProviderError::Transport("the embedder stopped".to_owned()))?
    }
}

pub(crate) struct OnnxReranker {
    models: Arc<Models>,
}

#[async_trait]
impl RerankClient for OnnxReranker {
    async fn rerank(&self, query: &str, docs: &[String]) -> Result<Vec<f64>, ProviderError> {
        let query = query.to_owned();
        let documents: Vec<String> = docs.iter().map(|d| plain_text(d)).collect();
        let models = self.models.clone();
        tokio::task::spawn_blocking(move || {
            let mut slot = models
                .reranker
                .lock()
                .map_err(|_| ProviderError::Transport("the reranker failed earlier".to_owned()))?;
            slot.last_used = Instant::now();
            if slot.model.is_none() {
                slot.model = Some(load_reranker(&models.dir, &models.manifest)?);
            }
            let model = slot
                .model
                .as_mut()
                .ok_or_else(|| ProviderError::Transport("no reranker".to_owned()))?;
            let refs: Vec<&str> = documents.iter().map(String::as_str).collect();
            let ranked = model
                .rerank(query.as_str(), refs.as_slice(), false, None)
                .map_err(|_| ProviderError::Malformed("the reranker failed".to_owned()))?;
            let mut scores = vec![0.0f64; documents.len()];
            for hit in ranked {
                if let Some(slot) = scores.get_mut(hit.index) {
                    *slot = probability(hit.score);
                }
            }
            Ok(scores)
        })
        .await
        .map_err(|_| ProviderError::Transport("the reranker stopped".to_owned()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_recall_embeds_as_a_query_and_everything_else_as_a_passage() {
        assert_eq!(input_prefix(), "passage: ");
        assert_eq!(as_recall_query(async { input_prefix() }).await, "query: ");
    }

    #[test]
    fn the_reranker_reads_the_statement_and_a_probability_comes_back() {
        assert_eq!(
            plain_text(r#"{"statement":"Rates are reviewed.","subject":"Rates"}"#),
            "Rates. Rates are reviewed."
        );
        assert_eq!(plain_text(r#"{"statement":"Plain."}"#), "Plain.");
        assert_eq!(plain_text("not json"), "not json");
        assert_eq!(plain_text(r#"{"other":1}"#), r#"{"other":1}"#);
        assert!((probability(0.0) - 0.5).abs() < 1e-9);
        assert!(probability(10.0) > 0.99);
        assert!(probability(-10.0) < 0.01);
    }

    fn bundle(dir: &Path) -> ModelManifest {
        let mut manifest = ModelManifest::default();
        for file in bundled_files() {
            let path = dir.join(&file);
            std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder");
            std::fs::write(&path, format!("bytes of {file}")).expect("the file");
            manifest.files.push(crate::config::ModelFileDigest {
                sha256: crate::service::file_sha256(&path).expect("a digest"),
                path: file,
            });
        }
        manifest
    }

    #[test]
    fn a_load_reads_the_checked_bytes_and_refuses_a_swapped_file() {
        let dir = std::env::temp_dir().join(format!("ordin-models-{}", uuid::Uuid::now_v7()));
        let manifest = bundle(&dir);
        let bytes = read_checked(&dir, &manifest, EMBEDDING_MODEL).expect("the file checks");
        assert_eq!(bytes, format!("bytes of {EMBEDDING_MODEL}").into_bytes());
        assert!(read_tokenizer(&dir, &manifest, "reranker").is_ok());

        // Swapped after the check at start, while the model sat unloaded.
        std::fs::write(dir.join(EMBEDDING_MODEL), b"another model").expect("the swap");
        std::fs::write(dir.join("reranker/tokenizer.json"), b"{}").expect("the swap");
        assert!(read_checked(&dir, &manifest, EMBEDDING_MODEL).is_err());
        assert!(read_tokenizer(&dir, &manifest, "reranker").is_err());
        assert!(read_tokenizer(&dir, &manifest, "embedding").is_ok());
        // A file the manifest does not list is no file of the bundle.
        assert!(read_checked(&dir, &ModelManifest::default(), RERANK_MODEL).is_err());
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[tokio::test]
    async fn a_folder_without_model_files_fails_the_call_and_never_panics() {
        let models = Arc::new(Models::new(
            PathBuf::from("/nonexistent/ordin-models"),
            ModelManifest::default(),
        ));
        let embedder = models.embedder();
        let reranker = models.reranker();
        assert!(embedder.embed(&["x".to_owned()]).await.is_err());
        assert!(reranker.rerank("q", &["d".to_owned()]).await.is_err());
        models.unload(true);
    }
}
