//! The test rig: the real Journal, the real memory component and this service over SQLite on a
//! temporary file, with the deterministic models of `tests/support/models.rs`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use memory::types::domain::Fact;
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalEntry, JournalOptions,
    MigrationRegistry, SystemClock, MIGRATIONS,
};
use ordin_knowledge_query_client::{KnowledgeAsk, KnowledgeBatch, KqError};
use ordin_mcp_front::{
    CallVia, ProgressSink, ServerName, ToolCallContext, ToolCaller, ToolFailure, ToolKey,
    ToolOutput,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::{KnowledgeHit, UnansweredSource};
use ordin_types::{
    AgentInstanceId, CrewRunId, KnowledgePageId, MemoryFactId, RunId, ScopeId, TeamFolderId,
    ToolRequestId, WorkspaceId,
};
use serde_json::Value;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::models::{WordEmbedder, WordReranker};
use crate::db::q;
use crate::model::*;
use crate::service::TestSeams;
use crate::wider::{WiderClient, WiderStream};
use crate::{MemoryError, MemoryService, MemoryServiceConfig, ModelManifest, ProgramKind};

/// A folder removed when dropped.
pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-memory-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) struct Rig {
    pub(crate) dir: TempDir,
    pub(crate) journal: Arc<Journal>,
    pub(crate) store: Datastore,
    pub(crate) svc: MemoryService,
    pub(crate) shutdown: CancellationToken,
    pub(crate) w1: WorkspaceId,
    pub(crate) w2: WorkspaceId,
    pub(crate) run: RunId,
    pub(crate) scope_a: ScopeId,
    pub(crate) scope_k: ScopeId,
}

pub(crate) struct Options {
    pub(crate) program: ProgramKind,
    pub(crate) rate: u32,
    pub(crate) embedder: Option<Arc<dyn memory::types::ports::EmbeddingClient>>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            program: ProgramKind::Desktop,
            rate: 60_000_000,
            embedder: None,
        }
    }
}

pub(crate) fn config(
    program: ProgramKind,
    a: Option<ScopeId>,
    k: Option<ScopeId>,
) -> MemoryServiceConfig {
    MemoryServiceConfig {
        program,
        models_dir: PathBuf::from("/nonexistent/ordin-models"),
        agent_wide_scope: a,
        log_level: tracing::Level::INFO,
        recall_log_days: 90,
        knowledge_scope: k,
        model_manifest: ModelManifest::default(),
    }
}

pub(crate) fn seams(rate: u32) -> TestSeams {
    TestSeams {
        embedder: Arc::new(WordEmbedder),
        reranker: Arc::new(WordReranker),
        rate_per_min: rate,
        reconcile_every: Duration::from_millis(20),
        recall_limit_secs: 10,
    }
}

pub(crate) async fn open(dir: &TempDir) -> (Datastore, Arc<Journal>) {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("data").join("journal.db"),
        backup_dir: dir.0.join("data").join("backups"),
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
    let journal = Journal::start(
        store.clone(),
        JournalOptions {
            automatic_pruning: false,
            prune_interval: Duration::from_secs(86_400),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .expect("the journal");
    (store, Arc::new(journal))
}

impl Rig {
    pub(crate) async fn desktop() -> Rig {
        Rig::with(Options::default()).await
    }

    pub(crate) async fn agent_profile() -> Rig {
        Rig::with(Options {
            program: ProgramKind::AgentSingle,
            ..Options::default()
        })
        .await
    }

    pub(crate) async fn with(options: Options) -> Rig {
        let dir = TempDir::new();
        let (store, journal) = open(&dir).await;
        let shutdown = CancellationToken::new();
        let scope_a = ScopeId::new();
        let scope_k = ScopeId::new();
        let agent = matches!(
            options.program,
            ProgramKind::AgentSingle | ProgramKind::AgentScaled
        );
        let mut test_seams = seams(options.rate);
        if let Some(embedder) = options.embedder {
            test_seams.embedder = embedder;
        }
        let svc = MemoryService::start_with(
            config(
                options.program,
                agent.then_some(scope_a),
                agent.then_some(scope_k),
            ),
            journal.clone(),
            shutdown.clone(),
            test_seams,
        )
        .await
        .expect("the service starts");
        Rig {
            dir,
            journal,
            store,
            svc,
            shutdown,
            w1: WorkspaceId::new(),
            w2: WorkspaceId::new(),
            run: RunId::new(),
            scope_a,
            scope_k,
        }
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    pub(crate) fn ordin(&self, w: WorkspaceId) -> MemoryCaller {
        MemoryCaller::Ordin {
            workspace_id: w,
            run_id: self.run,
        }
    }

    pub(crate) fn crew(&self, w: WorkspaceId, agent: AgentInstanceId) -> MemoryCaller {
        MemoryCaller::CrewMember {
            workspace_id: w,
            run_id: self.run,
            crew_run_id: CrewRunId::new(),
            agent_instance_id: agent,
        }
    }

    pub(crate) fn hosted(&self, scope: ScopeId) -> MemoryCaller {
        MemoryCaller::HostedAgent {
            scope_id: scope,
            run_id: RunId::new(),
        }
    }

    /// Ordin remembers `statement` in `w` and waits for it to be kept.
    pub(crate) async fn fact(&self, w: WorkspaceId, statement: &str) -> MemoryFactId {
        self.fact_about(w, statement, None).await
    }

    pub(crate) async fn fact_about(
        &self,
        w: WorkspaceId,
        statement: &str,
        about: Option<&str>,
    ) -> MemoryFactId {
        let receipt = self
            .svc
            .remember(self.ordin(w), input(statement, about))
            .await
            .expect("remember");
        self.kept(receipt.id).await;
        receipt.id
    }

    pub(crate) async fn row(&self, id: MemoryFactId) -> crate::store::RefRow {
        crate::store::ref_by_id(&self.svc.inner.db, id)
            .await
            .expect("read the row")
            .expect("a row")
    }

    pub(crate) async fn state(&self, id: MemoryFactId) -> FactState {
        self.row(id).await.state
    }

    pub(crate) async fn wait_state(&self, id: MemoryFactId, state: FactState) {
        let started = Instant::now();
        while self.state(id).await != state {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "row {id} never became {state:?}: {:?}",
                self.state(id).await
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Waits for `count` writes held in `w`, and keeps them as the person, oldest first.
    pub(crate) async fn keep_held_writes(&self, w: WorkspaceId, count: usize) -> Vec<MemoryFactId> {
        let place = MemoryPlace::Workspace(w);
        let started = Instant::now();
        let held = loop {
            let held = self.svc.held_writes(place).await.expect("held writes");
            if held.len() >= count {
                break held;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "only {} writes held",
                held.len()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let mut kept = Vec::new();
        for write in held.iter().rev() {
            let receipt = self
                .svc
                .keep_held(place, &write.id, &write.content_hash)
                .await
                .expect("keep");
            self.wait_state(receipt.id, FactState::Kept).await;
            kept.push(receipt.id);
        }
        self.settle().await;
        kept
    }

    pub(crate) async fn kept(&self, id: MemoryFactId) {
        self.wait_state(id, FactState::Kept).await;
        self.settle().await;
    }

    /// The component's fact behind a row, past every filter.
    pub(crate) async fn stored(&self, id: MemoryFactId) -> Option<Fact> {
        let row = self.row(id).await;
        let context = self.svc.inner.maintenance_context();
        self.svc
            .inner
            .storage
            .get_fact(&context, &row.memory_fact_id)
            .await
            .expect("read the fact")
    }

    pub(crate) async fn recall_text(&self, w: WorkspaceId, text: &str) -> RecallResult {
        self.svc
            .recall(self.ordin(w), query(text))
            .await
            .expect("recall")
    }

    pub(crate) async fn entries(&self, kind: &str) -> Vec<JournalEntry> {
        self.journal
            .list_entries(
                EntryFilter {
                    run_id: None,
                    kind: Some(kind.to_owned()),
                },
                None,
            )
            .await
            .expect("the journal")
            .items
    }

    pub(crate) async fn count(&self, sql: &str) -> i64 {
        let row = self
            .svc
            .inner
            .db
            .first(q(sql))
            .await
            .expect("the count")
            .expect("a row");
        row.int("n").expect("n")
    }

    pub(crate) async fn execute(&self, sql: &str) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(self.pool())
            .await
            .expect("the statement");
    }

    pub(crate) fn provider(&self) -> Arc<dyn ordin_mcp_front::ToolProvider> {
        self.svc.tool_provider()
    }

    /// One tool call as the MCP Front would make it, after the Rules Engine allowed it.
    pub(crate) async fn call(
        &self,
        tool: &str,
        caller: ToolCaller,
        w: WorkspaceId,
        agent: AgentInstanceId,
        scope_id: Option<ScopeId>,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        self.call_in_run(tool, caller, self.run, w, agent, scope_id, arguments)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn call_in_run(
        &self,
        tool: &str,
        caller: ToolCaller,
        run: RunId,
        w: WorkspaceId,
        agent: AgentInstanceId,
        scope_id: Option<ScopeId>,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        self.provider()
            .call(
                context(caller, run, w, agent, scope_id),
                &key(tool),
                arguments,
            )
            .await
    }

    pub(crate) async fn crew_call(
        &self,
        tool: &str,
        w: WorkspaceId,
        agent: AgentInstanceId,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        self.call(
            tool,
            ToolCaller::CrewMember {
                crew_run_id: CrewRunId::new(),
            },
            w,
            agent,
            None,
            arguments,
        )
        .await
    }

    pub(crate) async fn stop(self) {
        self.shutdown.cancel();
    }
}

pub(crate) fn context(
    caller: ToolCaller,
    run_id: RunId,
    workspace_id: WorkspaceId,
    agent_instance_id: AgentInstanceId,
    scope_id: Option<ScopeId>,
) -> ToolCallContext {
    ToolCallContext {
        run_id,
        workspace_id,
        agent_instance_id,
        caller,
        scope_id,
        coding_task_id: None,
        call_id: ToolRequestId::new(),
        correlation_id: "test".to_owned(),
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: CancellationToken::new(),
        progress: ProgressSink::detached(),
        only_needed_parts: false,
        via: CallVia::Harness,
    }
}

pub(crate) fn key(tool: &str) -> ToolKey {
    ToolKey {
        server: ServerName::new("memory").expect("the name"),
        tool: tool.to_owned(),
    }
}

pub(crate) fn input(statement: &str, about: Option<&str>) -> RememberInput {
    RememberInput {
        statement: statement.to_owned(),
        about: about.map(str::to_owned),
        fact_class: None,
        source: None,
        entities: Vec::new(),
        links: Vec::new(),
        made_from: Vec::new(),
    }
}

pub(crate) fn query(text: &str) -> RecallInput {
    RecallInput {
        query: Some(text.to_owned()),
        searches: None,
        folders: None,
        result_cap: 10,
        wider: WiderMode::Auto,
    }
}

pub(crate) fn thing(name: &str) -> ThingInput {
    ThingInput {
        name: name.to_owned(),
        other_names: Vec::new(),
    }
}

pub(crate) fn link(from: &str, kind: &str, to: &str) -> LinkInput {
    LinkInput {
        from: from.to_owned(),
        kind: kind.to_owned(),
        to: to.to_owned(),
        ended: false,
        confidence_permille: None,
    }
}

pub(crate) fn rules(retains: &[&str], excludes: &[&str], days: Option<u32>) -> AgentMemoryRules {
    AgentMemoryRules {
        retains: retains.iter().map(|s| (*s).to_owned()).collect(),
        excludes: excludes.iter().map(|s| (*s).to_owned()).collect(),
        max_age_days: days,
        store_location: "memory".to_owned(),
    }
}

pub(crate) fn code_of<T: std::fmt::Debug>(result: Result<T, MemoryError>) -> (&'static str, bool) {
    let error = result.expect_err("a failure");
    (error.code(), error.retryable())
}

pub(crate) fn statements(result: &RecallResult) -> Vec<String> {
    result.facts.iter().map(|f| f.statement.clone()).collect()
}

/// A page's sections from `(heading, text)`.
pub(crate) fn sections(items: &[(&str, &str)]) -> Vec<PageSection> {
    items
        .iter()
        .map(|(heading, text)| PageSection {
            heading: (*heading).to_owned(),
            text: (*text).to_owned(),
        })
        .collect()
}

pub(crate) fn graph(title: &str) -> PageGraph {
    PageGraph {
        title: title.to_owned(),
        other_names: Vec::new(),
        links_to: Vec::new(),
        standing: PageStanding::default(),
    }
}

impl Rig {
    /// Indexes a page and waits until its new rows are kept.
    pub(crate) async fn page(
        &self,
        w: WorkspaceId,
        page: KnowledgePageId,
        items: &[(&str, &str)],
        version: &str,
        graph: PageGraph,
    ) {
        self.svc
            .index_page_sections(w, page, sections(items), version.to_owned(), graph)
            .await
            .expect("index the page");
        self.settle().await;
    }

    /// Waits until no row is pending.
    pub(crate) async fn settle(&self) {
        let started = Instant::now();
        loop {
            let pending = self
                .count("SELECT COUNT(*) AS n FROM memory_fact_refs WHERE state = 'pending'")
                .await;
            let late = self
                .count(
                    "SELECT COUNT(*) AS n FROM memory_fact_refs WHERE state = 'replaced' \
                     AND settled_at IS NULL",
                )
                .await;
            // A row turns kept when its fact is stored; the job writes the fact's links after that.
            let jobs = self
                .count(
                    "SELECT COUNT(*) AS n FROM mem_work_job WHERE status IN ('pending', 'leased')",
                )
                .await;
            if pending == 0 && late == 0 && jobs == 0 {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "rows never settled"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// A folder service as the program implements it over the Team Knowledge Service.
#[derive(Default)]
pub(crate) struct FakeFolders {
    /// name → read folder (None = own).
    pub(crate) folders: Mutex<HashMap<String, Option<TeamFolderId>>>,
    pub(crate) descriptions: Mutex<HashMap<String, String>>,
    pub(crate) adopted: Mutex<HashMap<String, String>>,
}

impl FakeFolders {
    pub(crate) fn add(&self, name: &str, folder: Option<TeamFolderId>) {
        self.folders.lock().unwrap().insert(name.to_owned(), folder);
    }
}

#[async_trait]
impl MemoryFolders for FakeFolders {
    async fn collections_for(
        &self,
        _: WorkspaceId,
        names: &[String],
    ) -> Result<Vec<String>, String> {
        let folders = self.folders.lock().unwrap();
        names
            .iter()
            .map(|name| {
                if name == "own" {
                    return Ok(String::new());
                }
                match folders.get(name) {
                    Some(Some(folder)) => Ok(format!("tk:{folder}")),
                    Some(None) => Ok(String::new()),
                    None => Err(name.clone()),
                }
            })
            .collect()
    }

    async fn label_for(
        &self,
        _: WorkspaceId,
        read_folder_id: Option<TeamFolderId>,
        _: &str,
    ) -> Option<FolderLabel> {
        let folders = self.folders.lock().unwrap();
        let (name, _) = folders.iter().find(|(_, f)| **f == read_folder_id)?;
        Some(FolderLabel {
            description: self.descriptions.lock().unwrap().get(name).cloned(),
            name: name.clone(),
            set_by_company: false,
        })
    }

    async fn adoption_for(&self, _: WorkspaceId, item_id: &str) -> Option<AdoptionLabel> {
        self.adopted
            .lock()
            .unwrap()
            .get(item_id)
            .map(|from| AdoptionLabel {
                adopted_from: from.clone(),
                mark: None,
            })
    }
}

/// A Knowledge Query Client that records every `KnowledgeAsk` and answers from a script.
#[derive(Default)]
pub(crate) struct FakeClient {
    pub(crate) connections: Mutex<Vec<String>>,
    pub(crate) names: Mutex<Vec<String>>,
    pub(crate) asks: Mutex<Vec<RecordedAsk>>,
    pub(crate) fail: Mutex<bool>,
    pub(crate) first: Mutex<Option<KnowledgeBatch>>,
    /// Later batches, each after its delay.
    pub(crate) later: Mutex<Vec<(Duration, KnowledgeBatch)>>,
}

/// What a test reads back from an ask.
pub(crate) struct RecordedAsk {
    pub(crate) workspace: bool,
    pub(crate) query: Option<String>,
    pub(crate) searches: usize,
    pub(crate) limit: u8,
}

pub(crate) struct FakeStream {
    first: Option<KnowledgeBatch>,
    later: std::vec::IntoIter<(Duration, KnowledgeBatch)>,
}

#[async_trait]
impl WiderStream for FakeStream {
    async fn first(&mut self) -> KnowledgeBatch {
        self.first.take().unwrap_or_else(|| batch(Vec::new(), true))
    }

    async fn more(&mut self) -> Option<KnowledgeBatch> {
        let (delay, batch) = self.later.next()?;
        tokio::time::sleep(delay).await;
        Some(batch)
    }
}

#[async_trait]
impl WiderClient for FakeClient {
    async fn connection_names(&self, _: WorkspaceId) -> Vec<String> {
        self.connections.lock().unwrap().clone()
    }

    async fn contents_names(&self, _: WorkspaceId) -> Vec<String> {
        self.names.lock().unwrap().clone()
    }

    async fn ask(&self, ask: KnowledgeAsk) -> Result<Box<dyn WiderStream>, KqError> {
        self.asks.lock().unwrap().push(RecordedAsk {
            workspace: matches!(
                ask.target,
                ordin_knowledge_query_client::AskTarget::Workspace(_)
            ),
            query: ask.query,
            searches: ask.searches.map_or(0, |s| s.len()),
            limit: ask.limit,
        });
        if *self.fail.lock().unwrap() {
            return Err(KqError::Unreachable);
        }
        Ok(Box::new(FakeStream {
            first: self.first.lock().unwrap().take(),
            later: std::mem::take(&mut *self.later.lock().unwrap()).into_iter(),
        }))
    }
}

pub(crate) fn batch(hits: Vec<KnowledgeHit>, final_batch: bool) -> KnowledgeBatch {
    KnowledgeBatch {
        hits,
        answered: vec!["Buying knowledge".to_owned()],
        unanswered: Vec::<UnansweredSource>::new(),
        relied_on: Vec::new(),
        final_batch,
        unchecked_sources: false,
        pending: Vec::new(),
    }
}

pub(crate) fn hit(statement: &str, commit: &str) -> KnowledgeHit {
    KnowledgeHit {
        statement: statement.to_owned(),
        agent: "Buying knowledge".to_owned(),
        repository: "acme/buying".to_owned(),
        path: "suppliers/kestrel.md".to_owned(),
        commit: commit.to_owned(),
        section: None,
        page_title: None,
        kind: KnowledgeHitKind::Document,
        tier: None,
        status: PageStatus::Stable,
        may_be_out_of_date: false,
        as_of: Timestamp::now(),
        answer_id: None,
        shared_identity: None,
        things: Vec::new(),
        person_checked: false,
    }
}

impl MemoryService {
    pub(crate) fn set_fake_client(&self, client: Arc<FakeClient>) {
        *self.inner.wider.write().unwrap() = Some(client);
    }
}

/// `entity:<uuid>` of a test.
pub(crate) fn entity_id() -> String {
    format!("entity:{}", Uuid::now_v7())
}

/// A valid `TeamFact` with one entity, and that entity.
pub(crate) fn team_fact(statement: &str) -> (TeamFact, TeamEntity) {
    let entity = TeamEntity {
        format: 1,
        id: entity_id(),
        canonical_name: statement
            .split_whitespace()
            .next()
            .unwrap_or("Thing")
            .to_owned(),
        aliases: Vec::new(),
    };
    let now = Timestamp::now();
    (
        TeamFact {
            format: 1,
            id: MemoryFactId::new(),
            memory_fact_id: format!("fact:{}", Uuid::now_v7()),
            statement: statement.to_owned(),
            about: None,
            memory_class: "semantic".to_owned(),
            fact_class: None,
            origin_kind: "person".to_owned(),
            source_id: None,
            entities: vec![entity.id.clone()],
            valid_from: now,
            valid_to: None,
            confidence_permille: 1000,
            supersedes: None,
            superseded_by: None,
            derived_from: Vec::new(),
            written_at: now,
            shared_by: "Priya Shah".to_owned(),
            shared_at: now,
        },
        entity,
    )
}

pub(crate) fn one_batch(
    fact: TeamFact,
    entity: TeamEntity,
    read_folder: Option<TeamFolderId>,
) -> TeamImportBatch {
    TeamImportBatch {
        facts: vec![fact],
        entities: vec![entity],
        sources: Vec::new(),
        relationships: Vec::new(),
        read_folder,
    }
}

impl Rig {
    /// Gets one fact from a team folder.
    pub(crate) async fn got(
        &self,
        w: WorkspaceId,
        statement: &str,
        folder: Option<TeamFolderId>,
    ) -> MemoryFactId {
        let (fact, entity) = team_fact(statement);
        let id = fact.id;
        let outcome = self
            .svc
            .import_team(w, one_batch(fact, entity, folder))
            .await
            .expect("import");
        assert_eq!(outcome[0].result, TeamImportResult::Kept, "{statement}");
        id
    }
}
