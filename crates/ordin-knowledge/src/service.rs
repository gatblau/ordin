//! `KnowledgeService`: construction (A), the background tasks of `run`, and the helpers every
//! part shares (the page datastore, the journal, the workspace check).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ordin_journal::{Journal, ModuleMigrations, NewJournalEntry};
use ordin_mcp_front::ToolProvider;
use ordin_types::domain::places::{Workspace, WorkspaceState};
use ordin_types::time::Timestamp;
use ordin_types::{PlaceId, RunId, WorkspaceId};
use ordin_workspaces::WorkspaceError;
use sqlx::pool::PoolConnection;
use sqlx::{Sqlite, SqlitePool};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::agent::AgentState;
use crate::error::KnowledgeError;
use crate::model::{NotAdoptedReason, ProgramKind};
use crate::ports::{
    KnowledgeDeps, KnowledgeFiles, KnowledgeFolders, KnowledgeIdentity, KnowledgeIndex,
    KnowledgeIngest, KnowledgeMemory, KnowledgePush, KnowledgeReader, KnowledgeRules,
    KnowledgeWorkspaces,
};

/// The module name in the migration registry.
pub const MODULE: &str = "knowledge";

/// SA-KLINK-09.
pub const RENAME_REWRITE_DELAY: Duration = Duration::from_secs(10);
pub const REWRITE_ATTEMPTS: u32 = 3;
pub const MAX_LINKS_PER_PAGE: usize = 1_000;
pub const MAX_LINK_SUGGESTIONS: usize = 20;
/// Each direction in a tool result.
pub const MAX_TOOL_LINKS: usize = 100;
/// SA-OKF-09.
pub const INDEX_DEBOUNCE: Duration = Duration::from_secs(5);
pub const MAX_VERIFIED: usize = 20;
pub const DESCRIPTION_MAX: usize = 200;
/// A-28.
pub const MAX_PAGES_PER_WORKSPACE: u32 = 500;
/// SA-KREAD-05.
pub const MAX_READ_PAGES_PER_FOLDER: u32 = 2_000;
/// C-4: the budget of one freshness check.
pub(crate) const CHECK_BUDGET: Duration = Duration::from_secs(5);
/// F-4.
pub(crate) const INDEX_IDLE: Duration = Duration::from_secs(60);

pub struct KnowledgeService {
    pub(crate) program: ProgramKind,
    pub(crate) journal: Arc<Journal>,
    pub(crate) files: Arc<dyn KnowledgeFiles>,
    pub(crate) workspaces: Arc<dyn KnowledgeWorkspaces>,
    pub(crate) rules: Arc<dyn KnowledgeRules>,
    pub(crate) memory: Arc<dyn KnowledgeMemory>,
    pub(crate) push: Arc<dyn KnowledgePush>,
    pub(crate) folders: Arc<dyn KnowledgeFolders>,
    pub(crate) identity: Arc<dyn KnowledgeIdentity>,
    pub(crate) app_version: String,
    pub(crate) agent: Option<Arc<AgentState>>,
    /// Page writes, file changes and renames are applied one at a time.
    pub(crate) write_lock: tokio::sync::Mutex<()>,
    /// Wakes the memory-index task (B-7, E).
    pub(crate) index_wake: Notify,
    /// Workspaces whose `knowledge/index.md` is due, and when (AD-1).
    pub(crate) index_due: Mutex<HashMap<WorkspaceId, Instant>>,
    pub(crate) index_due_wake: Notify,
    /// Files of `knowledge/` that could not be adopted, until they change or go (R-5).
    pub(crate) not_adopted: Mutex<HashMap<(PlaceId, String), NotAdopted>>,
    /// `knowledge_index_not_ours` is logged once per file version (AD-2).
    pub(crate) index_not_ours: Mutex<HashSet<(WorkspaceId, u32)>>,
    /// The workspace of each own-folder place seen.
    // Filled from page rows, `workspaces.get` and `KnowledgeWorkspaces::list_not_in_bin`.
    pub(crate) place_workspaces: Mutex<HashMap<PlaceId, WorkspaceId>>,
    /// Places a File Service call answered `FILE_PLACE_NOT_FOUND` for (SA-KTEAM-03).
    pub(crate) missing_places: Mutex<HashSet<PlaceId>>,
    /// The last hourly pass over `stale_after` (F, ADR-112).
    pub(crate) stale_checked_at: Mutex<Option<Timestamp>>,
    pub(crate) provider: OnceLock<Arc<dyn ToolProvider>>,
}

/// A file not adopted: its workspace, the reason and the file version refused.
pub(crate) type NotAdopted = (WorkspaceId, NotAdoptedReason, u32);

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl KnowledgeService {
    /// This module's migrations (module name "knowledge"), for the Journal's registry (ruling R3).
    // spec: ordin-desktop/components/KnowledgeService#SA-RECON-07
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: MODULE,
            sqlite: &[
                (1, "pages", include_str!("sql/sqlite_v1.sql")),
                (2, "items", include_str!("sql/sqlite_v2.sql")),
                (
                    3,
                    "team knowledge columns",
                    include_str!("sql/sqlite_v3.sql"),
                ),
                (
                    4,
                    "page links, names and tags",
                    include_str!("sql/sqlite_v4.sql"),
                ),
                (
                    5,
                    "read pages from other teams' folders",
                    include_str!("sql/sqlite_v5.sql"),
                ),
                (
                    6,
                    "open knowledge format",
                    include_str!("sql/sqlite_v6.sql"),
                ),
                // spec: ordin-desktop/components/KnowledgeService#KS-TR-3
                (7, "sections as passages", include_str!("sql/sqlite_v7.sql")),
            ],
            // The Journal requires versions 1, 2, 3, … on each engine; version 1 is a no-op here
            // so the items step keeps version 2 on both engines (KnowledgeService.md, SA-KBUILT-09).
            postgres: &[
                (
                    1,
                    "pages (none on PostgreSQL)",
                    include_str!("sql/postgres_v1.sql"),
                ),
                (2, "items", include_str!("sql/postgres_v2.sql")),
            ],
        }
    }

    /// KNOW_PROFILE_MISMATCH, KNOW_BAD_PATTERN, KNOW_NO_NOTES_SOURCE (A-1).
    pub async fn new(
        program: ProgramKind,
        deps: KnowledgeDeps,
    ) -> Result<Arc<Self>, KnowledgeError> {
        if deps.agent.is_some() != program.is_agent() {
            let error = KnowledgeError::ProfileMismatch;
            tracing::error!(
                component = "knowledge",
                code = error.code(),
                "knowledge_start_failed"
            );
            return Err(error);
        }
        let agent = match deps.agent {
            Some(a) => Some(Arc::new(AgentState::new(a).inspect_err(|e| {
                tracing::error!(
                    component = "knowledge",
                    code = e.code(),
                    "knowledge_start_failed"
                );
            })?)),
            None => None,
        };
        let svc = Arc::new(KnowledgeService {
            program,
            journal: deps.journal,
            files: deps.files,
            workspaces: deps.workspaces,
            rules: deps.rules,
            memory: deps.memory,
            push: deps.push,
            folders: deps.folders,
            identity: deps.identity,
            app_version: deps.app_version,
            agent,
            write_lock: tokio::sync::Mutex::new(()),
            index_wake: Notify::new(),
            index_due: Mutex::new(HashMap::new()),
            index_due_wake: Notify::new(),
            not_adopted: Mutex::new(HashMap::new()),
            index_not_ours: Mutex::new(HashSet::new()),
            place_workspaces: Mutex::new(HashMap::new()),
            missing_places: Mutex::new(HashSet::new()),
            stale_checked_at: Mutex::new(None),
            provider: OnceLock::new(),
        });
        Ok(svc)
    }

    /// Starts the freshness listeners and the memory-index task (desktop and personal server);
    /// returns when `shutdown` is cancelled. In the agent profile it starts nothing (A-2).
    pub async fn run(self: Arc<Self>, shutdown: tokio_util::sync::CancellationToken) {
        if self.program.is_agent() {
            shutdown.cancelled().await;
            return;
        }
        let files = tokio::spawn(Arc::clone(&self).file_listener(shutdown.clone()));
        let origins = tokio::spawn(Arc::clone(&self).origin_listener(shutdown.clone()));
        let indexer = tokio::spawn(Arc::clone(&self).index_task(shutdown.clone()));
        let housekeeping = tokio::spawn(Arc::clone(&self).housekeeping_task(shutdown.clone()));
        // Start-up scan (R-6, AF), in the background; the screens are usable meanwhile.
        let scan = {
            let svc = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(e) = svc.startup_scan().await {
                    tracing::warn!(
                        component = "knowledge",
                        code = e.code(),
                        "knowledge_scan_failed"
                    );
                }
            })
        };
        shutdown.cancelled().await;
        for task in [files, origins, indexer, housekeeping, scan] {
            let _ = task.await;
        }
    }

    /// The provider of the MCP server "knowledge" (SA-RECON-05). Every call returns the same one.
    pub fn tool_provider(self: &Arc<Self>) -> Arc<dyn ToolProvider> {
        Arc::clone(self.provider.get_or_init(|| {
            Arc::new(crate::tools::KnowledgeTools {
                svc: Arc::clone(self),
            }) as Arc<dyn ToolProvider>
        }))
    }

    /// Home and Search's reads (SA-RECON-03).
    pub fn reader(self: &Arc<Self>) -> Arc<dyn KnowledgeReader> {
        Arc::new(crate::reader::Reader {
            svc: Arc::clone(self),
        })
    }

    /// Some in the agent profile only.
    pub fn index(&self) -> Option<Arc<dyn KnowledgeIndex>> {
        self.agent.as_ref().map(|a| {
            Arc::new(crate::agent::Index::new(
                Arc::clone(a),
                self.journal.datastore().clone(),
            )) as Arc<dyn KnowledgeIndex>
        })
    }

    /// Some in the agent profile only.
    pub fn ingest(&self) -> Option<Arc<dyn KnowledgeIngest>> {
        self.agent.as_ref().map(|a| {
            Arc::new(crate::ingest::Ingest {
                index: crate::agent::Index::new(Arc::clone(a), self.journal.datastore().clone()),
                state: Arc::clone(a),
            }) as Arc<dyn KnowledgeIngest>
        })
    }

    // ------------------------------------------------------------ shared helpers

    pub(crate) fn desktop_only(&self) -> Result<(), KnowledgeError> {
        if self.program.is_agent() {
            Err(KnowledgeError::ProfileMismatch)
        } else {
            Ok(())
        }
    }

    pub(crate) fn pool(&self) -> Result<&SqlitePool, KnowledgeError> {
        self.journal
            .datastore()
            .sqlite()
            .ok_or(KnowledgeError::DbUnavailable)
    }

    pub(crate) async fn conn(&self) -> Result<PoolConnection<Sqlite>, KnowledgeError> {
        Ok(self.pool()?.acquire().await?)
    }

    /// B-2: `WS_NOT_FOUND` or `in_bin` → `KNOW_WORKSPACE_NOT_FOUND`.
    pub(crate) async fn workspace(&self, id: WorkspaceId) -> Result<Workspace, KnowledgeError> {
        match self.workspaces.get(id).await {
            Ok(w) if w.state == WorkspaceState::InBin => Err(KnowledgeError::WorkspaceNotFound),
            Ok(w) => {
                lock(&self.place_workspaces).insert(w.own_folder_place_id, w.id);
                Ok(w)
            }
            Err(WorkspaceError::NotFound) => Err(KnowledgeError::WorkspaceNotFound),
            Err(_) => Err(KnowledgeError::DbUnavailable),
        }
    }

    /// Journal `knowledge.<name>` as `knowledge_<name>` (SA-RECON-07). A failed journal write is
    /// logged; the operation it records has already happened.
    pub(crate) async fn journal(
        &self,
        name: &str,
        workspace_id: Option<WorkspaceId>,
        run_id: Option<RunId>,
        summary: &str,
        data: serde_json::Value,
    ) {
        let entry = NewJournalEntry {
            kind: format!("knowledge_{name}"),
            component: "knowledge".to_owned(),
            run_id,
            crew_run_id: None,
            workspace_id,
            summary: summary.to_owned(),
            data,
        };
        if let Err(e) = self.journal.append_entry(entry).await {
            tracing::warn!(component = "knowledge", error = %e, "knowledge_journal_failed");
        }
    }

    pub(crate) fn wake_indexer(&self) {
        self.index_wake.notify_one();
    }

    /// AD-1: schedules the workspace's index after `INDEX_DEBOUNCE`; later changes push it back.
    pub(crate) fn schedule_index(&self, workspace: WorkspaceId) {
        lock(&self.index_due).insert(workspace, Instant::now() + INDEX_DEBOUNCE);
        self.index_due_wake.notify_one();
    }

    async fn housekeeping_task(self: Arc<Self>, shutdown: CancellationToken) {
        loop {
            let next = {
                let due = lock(&self.index_due);
                due.values().min().copied()
            };
            let wait = next
                .map(|n| n.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::from_secs(1))
                .min(Duration::from_secs(1));
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(wait) => {}
                _ = self.index_due_wake.notified() => {}
            }
            let ready: Vec<WorkspaceId> = {
                let mut due = lock(&self.index_due);
                let now = Instant::now();
                let ready: Vec<WorkspaceId> = due
                    .iter()
                    .filter(|(_, at)| **at <= now)
                    .map(|(w, _)| *w)
                    .collect();
                for w in &ready {
                    due.remove(w);
                }
                ready
            };
            for w in ready {
                self.write_workspace_index(w).await;
            }
            if let Err(e) = self.run_due_rewrites(Timestamp::now()).await {
                tracing::warn!(
                    component = "knowledge",
                    code = e.code(),
                    "knowledge_rewrite_pass_failed"
                );
            }
        }
    }
}
