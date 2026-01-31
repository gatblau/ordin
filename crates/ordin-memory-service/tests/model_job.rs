//! The model job (the user's decision of 2026-01-21): the real `OnnxEmbedder` and `OnnxReranker` with
//! the real int8 models. Not in `make check`; run by `make memory-model-test`. It FAILS, never skips,
//! when the files are absent. `ORDIN_TEST_MODEL_DIR` names the folder, with `embedding/` and
//! `reranker/` inside it; no model file is in this repository.
#![cfg(unix)]

mod support;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_memory_service::{
    MemoryCaller, MemoryService, MemoryServiceConfig, ModelFileDigest, ModelManifest, ProgramKind,
    RecallInput, RememberInput, WiderMode,
};
use ordin_types::{RunId, WorkspaceId};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

const FILES: [&str; 10] = [
    "embedding/model.onnx",
    "embedding/tokenizer.json",
    "embedding/config.json",
    "embedding/special_tokens_map.json",
    "embedding/tokenizer_config.json",
    "reranker/model.onnx",
    "reranker/tokenizer.json",
    "reranker/config.json",
    "reranker/special_tokens_map.json",
    "reranker/tokenizer_config.json",
];

fn model_dir() -> PathBuf {
    let dir = std::env::var("ORDIN_TEST_MODEL_DIR")
        .expect("ORDIN_TEST_MODEL_DIR must name the folder holding the model files");
    PathBuf::from(dir)
}

fn manifest(dir: &std::path::Path) -> ModelManifest {
    let files = FILES
        .iter()
        .map(|file| {
            let bytes = std::fs::read(dir.join(file))
                .unwrap_or_else(|error| panic!("the model file {file} is needed: {error}"));
            let digest = Sha256::digest(&bytes);
            ModelFileDigest {
                path: (*file).to_owned(),
                sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
            }
        })
        .collect();
    ModelManifest { files }
}

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
    type Writer = Log;
    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}

fn input(statement: &str, about: &str) -> RememberInput {
    RememberInput {
        statement: statement.to_owned(),
        about: Some(about.to_owned()),
        fact_class: None,
        source: None,
        entities: Vec::new(),
        links: Vec::new(),
        made_from: Vec::new(),
    }
}

fn query(text: &str) -> RecallInput {
    RecallInput {
        query: Some(text.to_owned()),
        searches: None,
        folders: None,
        result_cap: 5,
        wider: WiderMode::Never,
    }
}

async fn kept(store: &Datastore, count: i64) {
    let pool = store.sqlite().expect("SQLite");
    let started = Instant::now();
    loop {
        let (n,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM memory_fact_refs WHERE state = 'kept'")
                .fetch_one(pool)
                .await
                .expect("count");
        if n == count {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "only {n} of {count} facts were kept"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "model job: needs the model files"]
async fn a_real_remember_and_recall_with_the_real_models() {
    let dir = model_dir();
    let manifest = manifest(&dir);

    let log = Log::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(log.clone())
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("the log");

    let data = std::env::temp_dir().join(format!("ordin-memory-model-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&data).expect("a folder");
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data.join("journal.db"),
        backup_dir: data.join("backups"),
    })
    .await
    .expect("the datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("the journal");
    registry
        .register(MemoryService::migrations())
        .expect("this module");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("the migrations");
    let journal = Arc::new(
        Journal::start(
            store.clone(),
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(86_400),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("the journal"),
    );
    let (dir_again, manifest_again) = (dir.clone(), manifest.clone());

    let shutdown = CancellationToken::new();
    let started = Instant::now();
    let svc = MemoryService::start(
        MemoryServiceConfig {
            program: ProgramKind::Desktop,
            models_dir: dir,
            agent_wide_scope: None,
            log_level: tracing::Level::INFO,
            recall_log_days: 90,
            knowledge_scope: None,
            model_manifest: manifest,
        },
        journal.clone(),
        shutdown.clone(),
    )
    .await
    .expect("start");
    println!("start (digests of all files): {:?}", started.elapsed());
    svc.ready().await.expect("ready");

    let who = MemoryCaller::Ordin {
        workspace_id: WorkspaceId::new(),
        run_id: RunId::new(),
    };
    let first_remember = Instant::now();
    svc.remember(
        who,
        input("Kestrel's Q3 quote is valid until 30 November.", "Kestrel"),
    )
    .await
    .expect("remember");
    println!("remember receipt: {:?}", first_remember.elapsed());
    kept(&store, 1).await;
    println!(
        "first fact kept (embedder loaded): {:?}",
        first_remember.elapsed()
    );
    for (n, (statement, about)) in [
        (
            "The board meets on the first Tuesday of the month.",
            "Board",
        ),
        (
            "Le fournisseur Borealis livre les emballages le lundi.",
            "Borealis",
        ),
    ]
    .iter()
    .enumerate()
    {
        svc.remember(who, input(statement, about))
            .await
            .expect("remember");
        kept(&store, n as i64 + 2).await;
    }

    let cold = Instant::now();
    let result = svc
        .recall(who, query("When does the Kestrel quote expire?"))
        .await
        .expect("the first recall");
    let cold = cold.elapsed();
    println!(
        "first recall (reranker loads): {cold:?}, returned {:?}",
        result
            .facts
            .iter()
            .map(|f| (f.statement.clone(), f.relevance_permille))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        result.facts[0].statement,
        "Kestrel's Q3 quote is valid until 30 November."
    );
    assert!(cold < Duration::from_secs(10), "the recall limit: {cold:?}");

    let warm = Instant::now();
    let french = svc
        .recall(who, query("Which day does the packaging supplier deliver?"))
        .await
        .expect("a warm recall");
    println!(
        "warm recall: {:?}, returned {:?}",
        warm.elapsed(),
        french
            .facts
            .iter()
            .map(|f| (f.statement.clone(), f.relevance_permille))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        french.facts[0].statement,
        "Le fournisseur Borealis livre les emballages le lundi."
    );

    shutdown.cancel();
    let stopped = Instant::now();
    let text = loop {
        let text = String::from_utf8(log.0.lock().unwrap().clone()).expect("UTF-8");
        if text.contains("memory.stopped") {
            break text;
        }
        assert!(
            stopped.elapsed() < Duration::from_secs(10),
            "the service never stopped: {text}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    println!("stopped in {:?}", stopped.elapsed());
    let loaded: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("memory.model_loaded"))
        .collect();
    assert_eq!(loaded.len(), 2, "both models loaded: {text}");
    for line in loaded {
        println!("{line}");
    }
    assert!(text.contains("memory.model_unloaded"), "{text}");

    // Both models cold, as after 10 idle minutes: what the 10-second recall limit has to cover.
    let shutdown = CancellationToken::new();
    let svc = MemoryService::start(
        MemoryServiceConfig {
            program: ProgramKind::Desktop,
            models_dir: dir_again,
            agent_wide_scope: None,
            log_level: tracing::Level::INFO,
            recall_log_days: 90,
            knowledge_scope: None,
            model_manifest: manifest_again,
        },
        journal,
        shutdown.clone(),
    )
    .await
    .expect("restart");
    let cold = Instant::now();
    let result = svc
        .recall(who, query("When does the Kestrel quote expire?"))
        .await
        .expect("a recall with both models cold");
    let cold = cold.elapsed();
    println!("recall with both models cold: {cold:?}");
    assert_eq!(
        result.facts[0].statement,
        "Kestrel's Q3 quote is valid until 30 November."
    );
    assert!(cold < Duration::from_secs(10), "the recall limit: {cold:?}");
    shutdown.cancel();
    let _ = std::fs::remove_dir_all(&data);
}

/// The job fails when the folder is empty: the model test never skips quietly.
#[test]
#[ignore = "model job: needs the model files"]
#[should_panic(expected = "the model file")]
fn the_job_fails_when_a_model_file_is_absent() {
    manifest(std::path::Path::new("/nonexistent/ordin-models"));
}
