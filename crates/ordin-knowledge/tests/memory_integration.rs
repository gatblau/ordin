//! Integration (plan 01, Step 53): a page saved through the Knowledge Service is indexed by
//! section in the real Memory Service, and a recall cites the page. The Memory Service runs over
//! the same SQLite journal with the deterministic word embedder and reranker below, so the test is
//! fast and offline; the File, Workspace, Rules, Push, Folders and Identity ports are the fakes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS};
use ordin_knowledge::{KnowledgeMemory, KnowledgeService, ProgramKind, WritePage};
use ordin_memory_service::{
    ContentCheck, EmbeddingClient, MemoryCaller, MemoryError, MemoryService, MemoryServiceConfig,
    ModelManifest, PageGraph, PageSection, ProviderError, RecallInput, RerankClient, TestSeams,
    WiderMode,
};
use ordin_types::domain::places::KnowledgeAuthor;
use ordin_types::{KnowledgePageId, PlaceId, RunId, TeamFolderId, WorkspaceId};
use support::fakes::{journal_on, Fakes, TempDir};
use tokio_util::sync::CancellationToken;

const DIM: usize = 384;

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(str::to_owned)
        .collect()
}

fn bucket(word: &str) -> usize {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for b in word.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    (hash % DIM as u64) as usize
}

/// Texts sharing words are close: each word hashed into one dimension of a unit vector.
struct Words;

#[async_trait]
impl EmbeddingClient for Words {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0f32; DIM];
                for w in words(t) {
                    v[bucket(&w)] += 1.0;
                }
                let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if n > 0.0 {
                    v.iter_mut().for_each(|x| *x /= n);
                } else {
                    v[0] = 1.0;
                }
                v
            })
            .collect())
    }
}

#[async_trait]
impl RerankClient for Words {
    async fn rerank(&self, query: &str, docs: &[String]) -> Result<Vec<f64>, ProviderError> {
        let wanted = words(query);
        Ok(docs
            .iter()
            .map(|d| {
                let held = words(d);
                if wanted.is_empty() {
                    0.0
                } else {
                    wanted.iter().filter(|w| held.contains(w)).count() as f64 / wanted.len() as f64
                }
            })
            .collect())
    }
}

/// The program's implementation of `KnowledgeMemory`, a delegation to the real Memory Service.
/// The program's forwarding: every call goes to the Memory Service one for one (KNOW-REC-1).
struct Adapter {
    svc: MemoryService,
}

#[async_trait]
impl KnowledgeMemory for Adapter {
    async fn index_page_sections(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
        sections: Vec<PageSection>,
        version: String,
        graph: PageGraph,
    ) -> Result<(), MemoryError> {
        self.svc
            .index_page_sections(workspace_id, page_id, sections, version, graph)
            .await
    }
    async fn remove_page(
        &self,
        workspace_id: WorkspaceId,
        folder_id: Option<TeamFolderId>,
        page_id: KnowledgePageId,
    ) -> Result<(), MemoryError> {
        self.svc.remove_page(workspace_id, folder_id, page_id).await
    }
    async fn source_changed(
        &self,
        origin_ref: &str,
        current_version: &str,
    ) -> Result<u32, MemoryError> {
        self.svc.source_changed(origin_ref, current_version).await
    }
    async fn index_read_page_sections(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
        page_id: KnowledgePageId,
        sections: Vec<PageSection>,
        version: String,
        graph: PageGraph,
    ) -> Result<(), MemoryError> {
        self.svc
            .index_read_page_sections(workspace_id, folder_id, page_id, sections, version, graph)
            .await
    }
    async fn check_content(
        &self,
        workspace_id: WorkspaceId,
        text: &str,
    ) -> Result<ContentCheck, MemoryError> {
        self.svc.check_content(workspace_id, text.to_owned()).await
    }
}

/// The Knowledge Service over the real Memory Service, both running, with one workspace.
struct Running {
    _dir: TempDir,
    store: Datastore,
    memory: MemoryService,
    knowledge: Arc<KnowledgeService>,
    shutdown: CancellationToken,
    running: tokio::task::JoinHandle<()>,
    w: WorkspaceId,
}

async fn running() -> Running {
    let dir = TempDir::new();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("journal.db"),
        backup_dir: dir.0.join("backups"),
    })
    .await
    .unwrap();
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(MemoryService::migrations()).unwrap();
    registry.register(KnowledgeService::migrations()).unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let journal = journal_on(store.clone()).await;
    let shutdown = CancellationToken::new();
    let memory = MemoryService::start_with(
        MemoryServiceConfig {
            program: ordin_memory_service::ProgramKind::Desktop,
            models_dir: PathBuf::from("/nonexistent/ordin-models"),
            agent_wide_scope: None,
            log_level: tracing::Level::INFO,
            recall_log_days: 90,
            knowledge_scope: None,
            model_manifest: ModelManifest::default(),
        },
        journal.clone(),
        shutdown.clone(),
        TestSeams {
            embedder: Arc::new(Words),
            reranker: Arc::new(Words),
            rate_per_min: 60_000_000,
            reconcile_every: Duration::from_millis(20),
            recall_limit_secs: 10,
        },
    )
    .await
    .expect("the Memory Service starts");

    let fakes = Fakes::new();
    let place = PlaceId::new();
    fakes.files.add_place(place);
    let w = fakes.workspaces.add("Q3 supplier review", place);
    let mut deps = fakes.deps(journal, None);
    deps.memory = Arc::new(Adapter {
        svc: memory.clone(),
    });
    let knowledge = KnowledgeService::new(ProgramKind::Desktop, deps)
        .await
        .unwrap();
    let running = tokio::spawn(Arc::clone(&knowledge).run(shutdown.clone()));

    Running {
        _dir: dir,
        store,
        memory,
        knowledge,
        shutdown,
        running,
        w,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_page_saved_is_indexed_by_section_in_the_real_memory_service_and_recall_cites_it() {
    let Running {
        _dir,
        store,
        memory,
        knowledge,
        shutdown,
        running,
        w,
    } = running().await;
    let page = knowledge
        .write_page(
            KnowledgeAuthor::Ordin,
            WritePage {
                workspace_id: w,
                page_id: None,
                title: "How supplier rates are set".into(),
                body_markdown: "## Review\nRates are reviewed each quarter against the purchasing policy.\n\n## Exceptions\nKestrel has a fixed rate until March.".into(),
                sources: vec![],
                run_id: Some(RunId::new()),
                tags: None,
                aliases: None,
                page_type: None,
                description: None,
            },
        )
        .await
        .unwrap();

    let started = Instant::now();
    let cited = loop {
        let result = memory
            .recall(
                MemoryCaller::Ordin {
                    workspace_id: w,
                    run_id: RunId::new(),
                },
                RecallInput {
                    query: Some("rates reviewed quarter".into()),
                    searches: None,
                    folders: None,
                    result_cap: 10,
                    wider: WiderMode::Never,
                },
            )
            .await
            .expect("recall");
        if let Some(fact) = result.facts.iter().find(|f| f.page_id == Some(page)) {
            break fact.clone();
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the page's sections were never recalled"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(cited.page_section.as_deref(), Some("Review"));

    // The page row records the indexed version.
    let indexed: Option<i64> =
        sqlx::query_scalar("SELECT memory_indexed_version FROM knowledge_pages WHERE id = ?1")
            .bind(page.to_string())
            .fetch_one(store.sqlite().unwrap())
            .await
            .unwrap();
    assert_eq!(indexed, Some(1));
    shutdown.cancel();
    running.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_page_with_a_section_over_4000_characters_is_indexed_not_refused() {
    let Running {
        _dir,
        store,
        memory: _,
        knowledge,
        shutdown,
        running,
        w,
    } = running().await;
    // A section of 5,000 characters, some of them multi-byte, is over the Memory Service's
    // 4,000-character section limit (`MEM_PAGE_SECTION_INVALID`).
    let long = "Kestrel’s rate is fixed until March. ".repeat(140);
    assert!(long.chars().count() > 5_000);
    let page = knowledge
        .write_page(
            KnowledgeAuthor::Ordin,
            WritePage {
                workspace_id: w,
                page_id: None,
                title: "Supplier rate history".into(),
                body_markdown: format!(
                    "## History\n{long}\n\n## Review\nRates are reviewed each quarter."
                ),
                sources: vec![],
                run_id: Some(RunId::new()),
                tags: None,
                aliases: None,
                page_type: None,
                description: None,
            },
        )
        .await
        .unwrap();

    let started = Instant::now();
    loop {
        let (indexed, attempts): (Option<i64>, i64) = sqlx::query_as(
            "SELECT memory_indexed_version, memory_index_attempts FROM knowledge_pages WHERE id = ?1",
        )
        .bind(page.to_string())
        .fetch_one(store.sqlite().unwrap())
        .await
        .unwrap();
        assert_eq!(attempts, 0, "the Memory Service refused the page");
        if indexed == Some(1) {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the page was never indexed"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    shutdown.cancel();
    running.await.unwrap();
}
