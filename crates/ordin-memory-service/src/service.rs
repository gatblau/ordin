//! `MemoryService`: start, wiring, the caller contexts of *Internal Logic* §3 and the small helpers
//! every operation shares.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use memory::config::Config;
use memory::embedded::{self, EmbeddedDeps};
use memory::maintenance::{DecayConfig, MaintenanceWorker};
use memory::providers::LocalPiiDetector;
use memory::service::Service;
use memory::store::sql::{SqlPool, SqlStorage};
use memory::types::ports::{EmbeddingClient, RerankClient, StoragePort, StoreError};
use memory::types::scope::{FactAuthority, OpSet, ScopeContext, ScopeRef};
use memory::write_pipeline::WritePipeline;
use ordin_journal::{Datastore, Journal, ModuleMigrations, NewJournalEntry};
use ordin_mcp_front::ToolProvider;
use ordin_types::{AgentInstanceId, CrewRunId, MemoryRecallId, RunId, ScopeId, WorkspaceId};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::auth::InProcessAuthenticator;
use crate::config::{
    component_config, MemoryServiceConfig, ProgramKind, EMBED_DIM, IN_PROCESS_CLIENT,
    RECALL_LIMIT_SECS, RECONCILE_SECS, TENANT,
};
use crate::db::{q, Db};
use crate::error::MemoryError;
use crate::model::{AgentMemoryRules, MemoryCaller, MemoryFolders, MemoryPages, MemoryPlace};
use crate::providers::{check_models, Models};
use crate::wider::{HeldAnswer, WiderClient};

/// Stand-ins for what `start` cannot give a test: everyday tests run a deterministic embedder and
/// reranker so `cargo test` stays offline (the user's decision of 2026-01-21), and write faster than
/// a real caller. Not for production use.
#[doc(hidden)]
pub struct TestSeams {
    pub embedder: Arc<dyn EmbeddingClient>,
    pub reranker: Arc<dyn RerankClient>,
    pub rate_per_min: u32,
    pub reconcile_every: Duration,
    pub recall_limit_secs: u32,
}

/// The default recall-log period (SA-AMEM-10).
pub(crate) fn valid_recall_log_days(days: u16) -> bool {
    (1..=3650).contains(&days)
}

pub(crate) struct Inner {
    pub(crate) program: ProgramKind,
    pub(crate) db: Db,
    pub(crate) service: Arc<Service>,
    pub(crate) storage: Arc<dyn StoragePort>,
    pub(crate) pipeline: Arc<WritePipeline>,
    pub(crate) maintenance: Arc<MaintenanceWorker>,
    pub(crate) auth: Arc<InProcessAuthenticator>,
    pub(crate) journal: Arc<Journal>,
    pub(crate) config: Arc<Config>,
    pub(crate) decay: DecayConfig,
    pub(crate) agent_wide_scope: Option<ScopeId>,
    pub(crate) knowledge_scope: Option<ScopeId>,
    pub(crate) rules: RwLock<HashMap<AgentInstanceId, AgentMemoryRules>>,
    pub(crate) folders: RwLock<Option<Arc<dyn MemoryFolders>>>,
    pub(crate) pages: RwLock<Option<Arc<dyn MemoryPages>>>,
    pub(crate) wider: RwLock<Option<Arc<dyn WiderClient>>>,
    pub(crate) recall_log_days: AtomicU16,
    /// Settings › Memory (MEM-SET-1).
    pub(crate) settings: RwLock<crate::model::MemorySettings>,
    /// §1 step 5: every call refuses.
    pub(crate) dim_mismatch: AtomicBool,
    /// §1 step 3: recall and remember answer `MEM_MODEL_UNAVAILABLE` until a start or
    /// `recheck_models` finds every file (MEM-MD-2).
    pub(crate) models_missing: AtomicBool,
    /// MEM-MD-3: one `recheck_models` at a time.
    pub(crate) recheck: tokio::sync::Mutex<()>,
    /// MEM-MD-3: raised by one at each refusal of the model gate.
    pub(crate) models_wanted: tokio::sync::watch::Sender<u64>,
    /// Whether the write pipeline of §1 step 6 runs (MEM-MD-5).
    pub(crate) pipeline_running: AtomicBool,
    pub(crate) held: Mutex<HashMap<MemoryRecallId, HeldAnswer>>,
    pub(crate) quotas: Mutex<crate::held::WriteQuotas>,
    pub(crate) shutdown: CancellationToken,
    pub(crate) models: Option<Arc<Models>>,
    pub(crate) reconcile_every: Duration,
    pub(crate) provider: OnceLock<Arc<dyn ToolProvider>>,
}

/// Arc-backed; `Clone`.
#[derive(Clone)]
pub struct MemoryService {
    pub(crate) inner: Arc<Inner>,
}

impl MemoryService {
    /// The tables of *Data Model* §A and §B, for the Journal's registry. Module name
    /// "memory_service".
    // spec: ordin-desktop/components/MemoryService#SA-RECON-05
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: crate::MODULE_NAME,
            sqlite: crate::sql::sqlite_steps(),
            postgres: crate::sql::postgres_steps(),
        }
    }

    /// Builds the embedded memory component over the Journal's datastore, checks readiness, starts
    /// the background tasks and returns the service. Does not register tools.
    pub async fn start(
        config: MemoryServiceConfig,
        journal: Arc<Journal>,
        shutdown: CancellationToken,
    ) -> Result<MemoryService, MemoryError> {
        start_inner(config, journal, shutdown, None).await
    }

    /// `start` with the two models and the pace of [`TestSeams`]. Not for production use.
    #[doc(hidden)]
    pub async fn start_with(
        config: MemoryServiceConfig,
        journal: Arc<Journal>,
        shutdown: CancellationToken,
        seams: TestSeams,
    ) -> Result<MemoryService, MemoryError> {
        start_inner(config, journal, shutdown, Some(seams)).await
    }

    /// The `memory` MCP server's provider, for the program to register with the MCP Front
    /// (SA-RECON-02). Every call returns the same provider.
    // spec: ordin-desktop/components/MemoryService#SA-RECON-02
    pub fn tool_provider(&self) -> Arc<dyn ToolProvider> {
        self.inner
            .provider
            .get_or_init(|| Arc::new(crate::provider::MemoryToolProvider::new(self.inner.clone())))
            .clone()
    }

    /// §1 step 3 again, over the same `models_dir` and `model_manifest`: `check_models`, logging
    /// `memory.model_missing { file }` for each file that fails. All pass → `models_missing` is
    /// cleared, `memory.models_ready` is logged and `true` returned; otherwise `false` and nothing
    /// changes. Calls run one at a time; a second caller waits and gets its own result. With the test
    /// seams (no models), it returns `models_ready()` unchanged.
    // spec: ordin-desktop/components/MemoryService#MEM-MD-2
    // spec: ordin-desktop/components/MemoryService#MEM-MD-3
    //
    // The first re-check that passes starts the write pipeline once (MEM-MD-5).
    // spec: ordin-desktop/components/MemoryService#MEM-MD-5
    pub async fn recheck_models(&self) -> bool {
        let _one_at_a_time = self.inner.recheck.lock().await;
        let Some(models) = self.inner.models.clone() else {
            return self.models_ready();
        };
        let (dir, manifest) = models.source();
        if !check_models(dir, manifest).await {
            return false;
        }
        self.inner.models_missing.store(false, Ordering::SeqCst);
        tracing::info!(target: "ordin::memory", "memory.models_ready");
        if !self.inner.shutdown.is_cancelled() && !self.inner.dim_mismatch.load(Ordering::SeqCst) {
            let _ = crate::background::start_write_pipeline(&self.inner);
        }
        true
    }

    /// `true` when the last check (at start or by `recheck_models`) found every file.
    // spec: ordin-desktop/components/MemoryService#MEM-MD-3
    pub fn models_ready(&self) -> bool {
        !self.inner.models_missing.load(Ordering::SeqCst)
    }

    /// A counter raised by one each time recall or remember is refused with `MEM_MODEL_UNAVAILABLE`
    /// because a file is missing (not for a time-out). The desktop asks the person on a change
    /// (OrdinDesktop.md DSK-MD-5).
    // spec: ordin-desktop/components/MemoryService#MEM-MD-3
    pub fn models_wanted(&self) -> tokio::sync::watch::Receiver<u64> {
        self.inner.models_wanted.subscribe()
    }

    /// ADR-090 item 3: rules for one agent; replaces any earlier rules for it.
    pub fn set_agent_rules(
        &self,
        agent_instance_id: AgentInstanceId,
        rules: AgentMemoryRules,
    ) -> Result<(), MemoryError> {
        self.inner.gate()?;
        if let Some(days) = rules.max_age_days {
            if !(1..=36_500).contains(&days) {
                tracing::error!(target: "ordin::memory", "memory.rules_invalid");
                return Err(MemoryError::invalid("MEM_RULES_INVALID", "max_age_days"));
            }
        }
        self.inner
            .rules
            .write()
            .map_err(|_| MemoryError::internal())?
            .insert(agent_instance_id, rules);
        Ok(())
    }

    pub fn clear_agent_rules(&self, agent_instance_id: AgentInstanceId) {
        if let Ok(mut rules) = self.inner.rules.write() {
            rules.remove(&agent_instance_id);
        }
    }

    /// Wiring: the program passes its `MemoryFolders` before any recall (absent → no labels,
    /// "folders" refused).
    pub fn set_folders(&self, folders: Arc<dyn MemoryFolders>) {
        if let Ok(mut slot) = self.inner.folders.write() {
            *slot = Some(folders);
        }
    }

    /// Wiring, before any `memory.things` call.
    pub fn set_pages(&self, pages: Arc<dyn MemoryPages>) {
        if let Ok(mut slot) = self.inner.pages.write() {
            *slot = Some(pages);
        }
    }

    /// SA-AMEM-10: 1–3,650.
    pub fn set_recall_log_days(&self, days: u16) -> Result<(), MemoryError> {
        if !valid_recall_log_days(days) {
            return Err(MemoryError::invalid("MEM_LIMIT_INVALID", "days"));
        }
        self.inner.recall_log_days.store(days, Ordering::Relaxed);
        Ok(())
    }

    /// Desktop and personal server: wiring, before the first recall. Absent = no front door is ever
    /// asked.
    pub fn set_knowledge_client(
        &self,
        client: Arc<ordin_knowledge_query_client::KnowledgeQueryClient>,
    ) {
        if let Ok(mut slot) = self.inner.wider.write() {
            *slot = Some(Arc::new(crate::wider::RealClient(client)));
        }
    }

    /// Readiness for the start-up checks and the agent profile's readiness route.
    pub async fn ready(&self) -> Result<(), MemoryError> {
        self.inner.gate()?;
        self.inner.storage.ready().await.map_err(store_error_of)
    }
}

/// A store error met outside a `Service` call.
pub(crate) fn store_error_of(error: StoreError) -> MemoryError {
    match error {
        StoreError::Timeout => MemoryError::store_timeout(),
        StoreError::Unavailable(detail) if detail.contains("locked") => {
            MemoryError::store_timeout()
        }
        StoreError::Unavailable(_) => MemoryError::store_unavailable(),
        StoreError::NotFound => MemoryError::fact_not_found(),
        StoreError::PartialDelete { .. } => MemoryError::delete_unverified(),
        _ => MemoryError::internal(),
    }
}

async fn start_inner(
    config: MemoryServiceConfig,
    journal: Arc<Journal>,
    shutdown: CancellationToken,
    seams: Option<TestSeams>,
) -> Result<MemoryService, MemoryError> {
    let fail = |reason: &'static str| {
        tracing::error!(target: "ordin::memory", reason, "memory.start_failed");
        MemoryError::Refused {
            code: "MEM_STARTUP_FAILED",
        }
    };
    if !(1..=3650).contains(&config.recall_log_days) {
        return Err(fail("recall_log_days"));
    }
    // Step 1.
    let (pool, db) = match (journal.datastore(), config.program) {
        (Datastore::Postgres(pool), ProgramKind::AgentScaled) => {
            (SqlPool::Postgres(pool.clone()), Db::Postgres(pool.clone()))
        }
        (Datastore::Sqlite(pool), program) if program != ProgramKind::AgentScaled => {
            (SqlPool::Sqlite(pool.clone()), Db::Sqlite(pool.clone()))
        }
        _ => return Err(fail("datastore_mismatch")),
    };

    // Steps 2 and 3. A missing file does not stop the start.
    let (embedder, reranker, models, version_digest, missing, rate, reconcile_every, limit) =
        match seams {
            Some(seams) => (
                seams.embedder,
                seams.reranker,
                None,
                "000000000000".to_owned(),
                false,
                seams.rate_per_min,
                seams.reconcile_every,
                seams.recall_limit_secs,
            ),
            None => {
                let digest = embedding_digest(&config);
                let missing = !check_models(&config.models_dir, &config.model_manifest).await;
                let models = Arc::new(Models::new(
                    config.models_dir.clone(),
                    config.model_manifest.clone(),
                ));
                (
                    models.embedder() as Arc<dyn EmbeddingClient>,
                    models.reranker() as Arc<dyn RerankClient>,
                    Some(models),
                    digest,
                    missing,
                    600,
                    Duration::from_secs(RECONCILE_SECS),
                    RECALL_LIMIT_SECS,
                )
            }
        };
    let model_version = format!("multilingual-e5-small@{version_digest}");
    let mut component = component_config(config.log_level, model_version.clone(), rate);
    component.recall_time_limit_secs = limit;
    let component = Arc::new(component);

    // Step 4.
    let storage: Arc<dyn StoragePort> = Arc::new(
        SqlStorage::new(pool, EMBED_DIM)
            .with_queue_retry(component.job_max_attempts, component.job_backoff_base_ms),
    );
    let auth = Arc::new(InProcessAuthenticator::default());
    let built = embedded::build(
        component.clone(),
        EmbeddedDeps {
            storage,
            embedder,
            reranker,
            pii: Arc::new(LocalPiiDetector::new()),
            authenticator: auth.clone(),
        },
    );

    let inner = Arc::new(Inner {
        program: config.program,
        db,
        service: built.service,
        storage: built.storage,
        pipeline: built.write_pipeline,
        maintenance: built.maintenance,
        auth,
        journal,
        decay: DecayConfig {
            decay_k: component.decay_k,
            prune_retrievability: component.prune_retrievability,
        },
        config: component,
        agent_wide_scope: config.agent_wide_scope,
        knowledge_scope: config.knowledge_scope,
        rules: RwLock::default(),
        folders: RwLock::default(),
        pages: RwLock::default(),
        wider: RwLock::default(),
        recall_log_days: AtomicU16::new(config.recall_log_days),
        settings: RwLock::default(),
        dim_mismatch: AtomicBool::new(false),
        models_missing: AtomicBool::new(missing),
        recheck: tokio::sync::Mutex::new(()),
        models_wanted: tokio::sync::watch::Sender::new(0),
        pipeline_running: AtomicBool::new(false),
        held: Mutex::default(),
        quotas: Mutex::default(),
        shutdown,
        models,
        reconcile_every,
        provider: OnceLock::new(),
    });

    // Step 5.
    inner
        .storage
        .ensure_tenant_namespace(TENANT)
        .await
        .map_err(store_error_of)?;
    match inner.storage.ready().await {
        Ok(()) => {}
        Err(StoreError::Internal(detail)) if detail.contains("embedding") => {
            tracing::error!(target: "ordin::memory", reason = "dim_mismatch", "memory.start_failed");
            inner.dim_mismatch.store(true, Ordering::Relaxed);
            return Ok(MemoryService { inner });
        }
        Err(error) => return Err(store_error_of(error)),
    }

    // Step 6.
    crate::background::spawn(&inner);

    // Step 7.
    let facts_total = crate::store::count(
        &inner.db,
        q("SELECT COUNT(*) AS n FROM memory_fact_refs WHERE state IN ('pending', 'kept')"),
    )
    .await?;
    tracing::info!(
        target: "ordin::memory",
        program = inner.program.as_str(),
        facts_total,
        model_version = model_version.as_str(),
        "memory.started"
    );
    Ok(MemoryService { inner })
}

fn embedding_digest(config: &MemoryServiceConfig) -> String {
    config
        .model_manifest
        .digest_of(crate::providers::EMBEDDING_MODEL)
        .unwrap_or_default()
        .chars()
        .take(12)
        .collect()
}

/// A correlation id for a call that did not come with one.
pub(crate) fn new_correlation_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

impl Inner {
    /// §1 step 5: a store whose embeddings do not fit refuses every call.
    pub(crate) fn gate(&self) -> Result<(), MemoryError> {
        if self.dim_mismatch.load(Ordering::Relaxed) {
            return Err(MemoryError::Refused {
                code: "MEM_DIM_MISMATCH",
            });
        }
        Ok(())
    }

    /// §1 step 3: no model file, no recall and no remember. Each refusal raises `models_wanted`
    /// (MEM-MD-3); a time-out is refused elsewhere and does not.
    // spec: ordin-desktop/components/MemoryService#MEM-MD-3
    pub(crate) fn model_gate(&self) -> Result<(), MemoryError> {
        self.gate()?;
        if self.models_missing.load(Ordering::Relaxed) {
            self.models_wanted
                .send_modify(|wanted| *wanted = wanted.wrapping_add(1));
            return Err(MemoryError::Unavailable {
                code: "MEM_MODEL_UNAVAILABLE",
                retryable: false,
            });
        }
        Ok(())
    }

    pub(crate) fn is_workspace_program(&self) -> bool {
        matches!(
            self.program,
            ProgramKind::Desktop | ProgramKind::PersonalServer
        )
    }

    pub(crate) fn folders(&self) -> Option<Arc<dyn MemoryFolders>> {
        self.folders.read().ok()?.clone()
    }

    pub(crate) fn pages(&self) -> Option<Arc<dyn MemoryPages>> {
        self.pages.read().ok()?.clone()
    }

    pub(crate) fn wider_client(&self) -> Option<Arc<dyn WiderClient>> {
        self.wider.read().ok()?.clone()
    }

    pub(crate) fn rules_of(&self, agent: AgentInstanceId) -> Option<AgentMemoryRules> {
        self.rules.read().ok()?.get(&agent).cloned()
    }

    /// §3: the `ScopeRef` a place's writes carry.
    pub(crate) fn owner_of(place: MemoryPlace) -> ScopeRef {
        let key = crate::store::scope_key(place);
        ScopeRef {
            tenant: TENANT.to_owned(),
            team: Some(key.clone()),
            user: key,
        }
    }

    fn context(
        &self,
        user: String,
        teams: Vec<String>,
        allowed_ops: OpSet,
        fact_authority: Option<Vec<FactAuthority>>,
    ) -> ScopeContext {
        ScopeContext {
            tenant: TENANT.to_owned(),
            teams,
            user,
            token_jti: String::new(),
            allowed_ops,
            client: Some(IN_PROCESS_CLIENT.to_owned()),
            fact_authority,
            correlation_id: String::new(),
        }
    }

    /// The read teams of a place: its own, and for a hosted agent's scope the agent-wide one.
    fn teams_of(&self, place: MemoryPlace) -> Vec<String> {
        let own = crate::store::scope_key(place);
        let mut teams = vec![own];
        if let (MemoryPlace::Scope(scope), Some(wide)) = (place, self.agent_wide_scope) {
            if scope != wide && Some(scope) != self.knowledge_scope {
                teams.push(crate::store::scope_key(MemoryPlace::Scope(wide)));
            }
        }
        teams
    }

    /// §3, the Person row: read, write and forget. Also the context of this service's own
    /// retirements and deletions, which act for the place's owner.
    pub(crate) fn place_context(&self, place: MemoryPlace) -> ScopeContext {
        self.context(
            crate::store::scope_key(place),
            self.teams_of(place),
            OpSet {
                read: true,
                write: true,
                forget: true,
            },
            None,
        )
    }

    /// §3: the context of a caller's own call; `retains` become the component's `fact_authority`
    /// (§4 step 2).
    pub(crate) fn caller_context(
        &self,
        who: &MemoryCaller,
        retains: Option<&[String]>,
    ) -> ScopeContext {
        let place = place_of(who);
        let forget = matches!(who, MemoryCaller::Person { .. });
        let authority = retains.filter(|r| !r.is_empty()).map(|retains| {
            retains
                .iter()
                .map(|class| FactAuthority {
                    class: class.clone(),
                    source: None,
                })
                .collect()
        });
        self.context(
            crate::store::scope_key(place),
            self.teams_of(place),
            OpSet {
                read: true,
                write: true,
                forget,
            },
            authority,
        )
    }

    /// §6: the memory component's own maintenance context, which reads the whole tenant.
    pub(crate) fn maintenance_context(&self) -> ScopeContext {
        ScopeContext {
            tenant: TENANT.to_owned(),
            teams: Vec::new(),
            user: String::new(),
            token_jti: "maintenance".to_owned(),
            allowed_ops: OpSet {
                read: true,
                write: true,
                forget: true,
            },
            client: None,
            fact_authority: None,
            correlation_id: new_correlation_id(),
        }
    }

    /// One `memory_<name>` journal record. The fact, thing or link it records has already changed,
    /// so a Journal failure is logged and does not fail the call.
    pub(crate) async fn journal(
        &self,
        name: &str,
        run_id: Option<RunId>,
        crew_run_id: Option<CrewRunId>,
        workspace_id: Option<WorkspaceId>,
        summary: &str,
        data: Value,
    ) {
        let entry = NewJournalEntry {
            kind: format!("memory_{name}"),
            component: "memory_service".to_owned(),
            run_id,
            crew_run_id,
            workspace_id,
            summary: summary.to_owned(),
            data,
        };
        if let Err(error) = self.journal.append_entry(entry).await {
            tracing::warn!(
                target: "ordin::memory",
                kind = name,
                code = ordin_types::CodedError::code(&error),
                "memory.journal_failed"
            );
        }
    }
}

/// §3: where a caller's facts live.
pub(crate) fn place_of(who: &MemoryCaller) -> MemoryPlace {
    match who {
        MemoryCaller::Ordin { workspace_id, .. }
        | MemoryCaller::CrewMember { workspace_id, .. }
        | MemoryCaller::Person { workspace_id }
        | MemoryCaller::KnowledgeService { workspace_id }
        | MemoryCaller::Agent { workspace_id, .. } => MemoryPlace::Workspace(*workspace_id),
        MemoryCaller::HostedAgent { scope_id, .. } | MemoryCaller::KnowledgeAgent { scope_id } => {
            MemoryPlace::Scope(*scope_id)
        }
    }
}

/// Reads a bundled file's digest input; shared by the check and its tests.
pub(crate) fn file_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(ordin_types::num::Sha256Hex::from_digest(&hasher.finalize().into()).0)
}
