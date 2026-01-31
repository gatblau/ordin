//! MemoryService.md amendment 2026-01-28 (MEM-MD-1 to MEM-MD-4): models fetched after start, one test
//! per added scenario, named after it.

use std::sync::Arc;

use async_trait::async_trait;
use memory::types::ports::{EmbeddingClient, ProviderError};

use super::acceptance::Captured;
use super::support::*;
use crate::config::{ModelFileDigest, ModelManifest, ProgramKind};
use crate::MemoryService;

/// Writes the ten files §1 step 3 checks into `dir` and returns their manifest.
fn ten_files(dir: &std::path::Path) -> ModelManifest {
    let mut files = Vec::new();
    for file in crate::providers::bundled_files() {
        let path = dir.join(&file);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        std::fs::write(&path, format!("the bytes of {file}")).expect("a file");
        files.push(ModelFileDigest {
            sha256: crate::service::file_sha256(&path).expect("a digest"),
            path: file,
        });
    }
    ModelManifest { files }
}

/// The real `start` over an empty `models_dir`, with `manifest`.
async fn started_without_files(
    rig_dir: &TempDir,
    models_dir: &std::path::Path,
    manifest: ModelManifest,
) -> (MemoryService, tokio_util::sync::CancellationToken) {
    let (_store, journal) = open(rig_dir).await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let mut config = config(ProgramKind::Desktop, None, None);
    config.models_dir = models_dir.to_path_buf();
    config.model_manifest = manifest;
    let svc = MemoryService::start(config, journal, shutdown.clone())
        .await
        .expect("the service still starts");
    (svc, shutdown)
}

/// The gate's refusal: `MEM_MODEL_UNAVAILABLE`, not retryable.
fn refused_by_the_gate<T: std::fmt::Debug>(result: Result<T, crate::MemoryError>) -> bool {
    matches!(result, Err(ref e) if e.code() == "MEM_MODEL_UNAVAILABLE" && !e.retryable())
}

// Scenario: Happy path — files that arrive after start are seen without a restart
#[tokio::test]
async fn happy_path_files_that_arrive_after_start_are_seen_without_a_restart() {
    let rig_dir = TempDir::new();
    let staging = TempDir::new();
    let manifest = ten_files(&staging.0);
    let models_dir = rig_dir.0.join("models");
    let (svc, shutdown) = started_without_files(&rig_dir, &models_dir, manifest).await;
    let w = ordin_types::WorkspaceId::new();
    let caller = crate::model::MemoryCaller::Ordin {
        workspace_id: w,
        run_id: ordin_types::RunId::new(),
    };
    assert!(!svc.models_ready());
    assert!(refused_by_the_gate(
        svc.recall(caller, query("Kestrel")).await
    ));

    std::fs::rename(&staging.0, &models_dir).expect("the files placed");
    assert!(svc.recheck_models().await);
    assert!(svc.models_ready());
    // The gate no longer refuses, so the next recall goes on to load the models from disk. These
    // test files are not real models, so the load itself is left to the behavioural check.
    assert!(svc.inner.model_gate().is_ok());
    shutdown.cancel();
}

// Scenario: Error path — a re-check with one wrong file changes nothing
#[tokio::test]
async fn error_path_a_recheck_with_one_wrong_file_changes_nothing() {
    let rig_dir = TempDir::new();
    let models_dir = rig_dir.0.join("models");
    let mut manifest = ten_files(&models_dir);
    let wrong = manifest
        .files
        .iter_mut()
        .find(|f| f.path == "reranker/tokenizer.json")
        .expect("the file");
    wrong.sha256 = "0".repeat(64);
    let (svc, shutdown) = started_without_files(&rig_dir, &models_dir, manifest).await;

    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish();
    let checked = {
        let _default = tracing::subscriber::set_default(subscriber);
        svc.recheck_models().await
    };
    assert!(!checked);
    assert!(!svc.models_ready());
    let lines = captured.0.lock().expect("lines").join("\n");
    assert!(lines.contains("memory.model_missing"), "{lines}");
    assert!(lines.contains("reranker/tokenizer.json"), "{lines}");
    assert!(!lines.contains("memory.models_ready"), "{lines}");
    let caller = crate::model::MemoryCaller::Ordin {
        workspace_id: ordin_types::WorkspaceId::new(),
        run_id: ordin_types::RunId::new(),
    };
    assert!(refused_by_the_gate(
        svc.recall(caller, query("Kestrel")).await
    ));
    shutdown.cancel();
}

/// An embedder that always times out, so a recall is refused with the retryable code.
struct TimesOut;

#[async_trait]
impl EmbeddingClient for TimesOut {
    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        Err(ProviderError::Timeout)
    }
}

// Scenario: Edge case — a refused use raises models_wanted
#[tokio::test]
async fn edge_case_a_refused_use_raises_models_wanted() {
    let rig = Rig::desktop().await;
    let mut wanted = rig.svc.models_wanted();
    assert_eq!(*wanted.borrow_and_update(), 0);
    rig.svc
        .inner
        .models_missing
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let refused = rig.svc.remember(rig.ordin(rig.w1), input("x", None)).await;
    assert_eq!(code_of(refused), ("MEM_MODEL_UNAVAILABLE", false));
    assert!(wanted.has_changed().expect("the sender lives"));
    assert_eq!(*wanted.borrow_and_update(), 1);
    rig.stop().await;

    // A recall that times out (retryable) does not raise it.
    let rig = Rig::with(Options {
        embedder: Some(Arc::new(TimesOut)),
        ..Options::default()
    })
    .await;
    let mut wanted = rig.svc.models_wanted();
    let timed_out = rig.svc.recall(rig.ordin(rig.w1), query("Kestrel")).await;
    assert_eq!(code_of(timed_out), ("MEM_MODEL_UNAVAILABLE", true));
    assert!(!wanted.has_changed().expect("the sender lives"));
    assert_eq!(*wanted.borrow_and_update(), 0);
    rig.stop().await;
}

// MEM-MD-3: with the test seams there are no models, so a re-check answers `models_ready()`.
#[tokio::test]
async fn with_the_test_seams_a_recheck_answers_models_ready_unchanged() {
    let rig = Rig::desktop().await;
    assert!(rig.svc.models_ready());
    assert!(rig.svc.recheck_models().await);
    rig.svc
        .inner
        .models_missing
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(!rig.svc.recheck_models().await);
    rig.stop().await;
}

// MEM-MD-4: the person's sentence for the code.
#[test]
fn the_sentence_for_model_unavailable_points_at_the_download() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ordin-localisation/catalogues/en-GB/errors-memory-service.ftl"
    ))
    .expect("the catalogue");
    assert!(source.contains(
        "error-mem-model-unavailable = Memory isn't ready on this computer yet. Choose Download when ordin offers it; if that keeps failing, tell IT."
    ));
}
