//! The service itself: construction, start-up, the 15-minute timer and the workspace lifecycle
//! (WorkspaceService.md Internal Logic 1 and 2).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ordin_files::{AddPlace, FileError, PlaceKind};
use ordin_journal::{Journal, NewJournalEntry};
use ordin_types::domain::places::{FolderGrant, Workspace, WorkspaceState};
use ordin_types::time::Timestamp;
use ordin_types::wire::events::EventKind;
use ordin_types::{ConfluenceLinkId, PlaceId, SourceLinkId, WorkingCopyItemId, WorkspaceId};
use sqlx::{Row, SqlitePool};
use tokio::sync::{broadcast, Semaphore};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::error::{file_failed, WorkspaceError};
use crate::log;
use crate::store::{self, to_text};
use crate::types::{BinImpact, LinkProgress, OriginChanged, WorkspaceDeps, WorkspaceListFilter};
use crate::{CHECK_CONCURRENCY, CHECK_INTERVAL};

/// The Workspace Service. Constructed by [`WorkspaceService::new`]; the program registers it with
/// `Journal::subscribe` (name `"workspaces"`) as the `EventSink` of Internal Logic 8.3.
pub struct WorkspaceService {
    pub(crate) me: Weak<WorkspaceService>,
    pub(crate) pool: SqlitePool,
    pub(crate) journal: Journal,
    pub(crate) rules: Arc<ordin_rules::RulesEngine>,
    pub(crate) files: Arc<ordin_files::FileService>,
    pub(crate) sharepoint: Arc<ordin_sharepoint::SharePointAdapter>,
    pub(crate) confluence: Option<Arc<ordin_confluence::ConfluenceAdapter>>,
    pub(crate) jira: Option<Arc<ordin_jira::JiraAdapter>>,
    pub(crate) data_root: PathBuf,
    pub(crate) documents_dir: PathBuf,
    pub(crate) on_server: bool,
    pub(crate) cancel: CancellationToken,
    pub(crate) tasks: Mutex<Vec<JoinHandle<()>>>,
    pub(crate) wc_tx: broadcast::Sender<WorkingCopyItemId>,
    pub(crate) origin_tx: broadcast::Sender<OriginChanged>,
    pub(crate) opened_tx: broadcast::Sender<WorkspaceId>,
    pub(crate) deleted_tx: broadcast::Sender<WorkspaceId>,
    pub(crate) removed_tx: broadcast::Sender<(WorkspaceId, SourceLinkId)>,
    pub(crate) progress: Mutex<HashMap<SourceLinkId, LinkProgress>>,
    pub(crate) checking: Mutex<HashSet<SourceLinkId>>,
    pub(crate) check_slots: Arc<Semaphore>,
    pub(crate) unpublished: Mutex<HashSet<WorkspaceId>>,
    pub(crate) reported_removed: Mutex<HashSet<WorkingCopyItemId>>,
    pub(crate) syncing: Mutex<HashSet<ConfluenceLinkId>>,
    pub(crate) sync_changes: Mutex<HashMap<ConfluenceLinkId, tokio::time::Instant>>,
    /// Results-folder paths that break WS-TW-9 and were logged `result_path_not_shared` once.
    pub(crate) results_not_shared: Mutex<crate::team::NotShared>,
}

impl WorkspaceService {
    /// Fails with `WS_DATASTORE_UNAVAILABLE` when the Journal's datastore is not SQLite (this module
    /// does not run in the agent profile).
    // spec: ordin-desktop/components/WorkspaceService#SA-RECON-03
    pub async fn new(deps: WorkspaceDeps) -> Result<Arc<WorkspaceService>, WorkspaceError> {
        let pool = deps
            .journal
            .datastore()
            .sqlite()
            .cloned()
            .ok_or(WorkspaceError::DatastoreUnavailable)?;
        Ok(Arc::new_cyclic(|me| WorkspaceService {
            me: me.clone(),
            pool,
            journal: deps.journal,
            rules: deps.rules,
            files: deps.files,
            sharepoint: deps.sharepoint,
            confluence: deps.confluence,
            jira: deps.jira,
            data_root: deps.data_root,
            documents_dir: deps.documents_dir,
            on_server: deps.on_server,
            cancel: CancellationToken::new(),
            tasks: Mutex::new(Vec::new()),
            wc_tx: broadcast::channel(1024).0,
            origin_tx: broadcast::channel(1024).0,
            opened_tx: broadcast::channel(256).0,
            deleted_tx: broadcast::channel(256).0,
            removed_tx: broadcast::channel(256).0,
            progress: Mutex::new(HashMap::new()),
            checking: Mutex::new(HashSet::new()),
            check_slots: Arc::new(Semaphore::new(CHECK_CONCURRENCY)),
            unpublished: Mutex::new(HashSet::new()),
            reported_removed: Mutex::new(HashSet::new()),
            syncing: Mutex::new(HashSet::new()),
            sync_changes: Mutex::new(HashMap::new()),
            results_not_shared: Mutex::new(HashSet::new()),
        }))
    }

    /// Publishes every non-bin workspace's grants, watches their places, tracks links without state,
    /// runs the start-up origin check, subscribes to File Service changes, starts the 15-minute timer.
    // spec: ordin-desktop/components/WorkspaceService#SA-RECON-04
    pub async fn start(self: &Arc<Self>) -> Result<(), WorkspaceError> {
        // 1.1: grants.
        let workspaces = self.workspaces_not_in_bin().await?;
        for workspace in &workspaces {
            if self.publish_with_backoff(workspace.id).await.is_err() {
                log::grants_publish_failed(workspace.id);
                self.mark_unpublished(workspace.id);
            }
        }
        // 1.2: watch every place of active and archived workspaces.
        for workspace in &workspaces {
            let places = self.place_ids_of(workspace.id).await?;
            if let Err(error) = self.files.watch(&places).await {
                log::background_failed("watch", error.code());
            }
        }
        // 1.4: File Service changes, before the first origin check so none is missed.
        let changes = self.files.subscribe_changes();
        let service = Arc::clone(self);
        self.spawn(async move { service.follow_place_changes(changes).await });
        // 1.3: a link the adapter holds no tracking state for is tracked by the first origin check
        // that reports `*_NOT_TRACKED`; the start-up origin check runs in the background.
        let service = Arc::clone(self);
        self.spawn(async move {
            service.check_all_active().await;
            service.retry_forgotten().await;
        });
        // 1.4: the timer.
        let service = Arc::clone(self);
        self.spawn(async move { service.timer().await });
        Ok(())
    }

    /// Stops the timer and every background task.
    pub async fn shutdown(&self) {
        self.cancel.cancel();
        let tasks: Vec<JoinHandle<()>> = lock(&self.tasks).drain(..).collect();
        for task in tasks {
            let _ = task.await;
        }
    }

    pub(crate) fn spawn<F>(&self, task: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let cancel = self.cancel.clone();
        let handle = tokio::spawn(async move {
            tokio::select! {
                () = cancel.cancelled() => {}
                () = task => {}
            }
        });
        let mut tasks = lock(&self.tasks);
        tasks.retain(|t| !t.is_finished());
        tasks.push(handle);
    }

    async fn timer(self: Arc<Self>) {
        loop {
            tokio::time::sleep(CHECK_INTERVAL).await;
            self.retry_unpublished().await;
            self.check_all_active().await;
        }
    }

    async fn check_all_active(self: &Arc<Self>) {
        let Ok(workspaces) = self.list(WorkspaceListFilter::Active).await else {
            return;
        };
        for workspace in workspaces {
            let _ = self.check_workspace(workspace.id).await;
        }
    }

    // ---- shared helpers

    /// "Journal `k {fields}`" of Internal Logic 1: a failed append is logged and does not undo the
    /// change.
    pub(crate) async fn record(
        &self,
        kind: &str,
        workspace_id: Option<WorkspaceId>,
        summary: &str,
        data: serde_json::Value,
    ) {
        let appended = self
            .journal
            .append_entry(NewJournalEntry {
                kind: kind.to_owned(),
                component: log::COMPONENT.to_owned(),
                run_id: None,
                crew_run_id: None,
                workspace_id,
                summary: summary.to_owned(),
                data,
            })
            .await;
        if appended.is_err() {
            log::journal_entry_failed(kind);
        }
    }

    /// Internal Logic 11: `workspace_changed { workspace_id }`, once per affected workspace.
    // spec: ordin-desktop/components/WorkspaceService#SA-RECON-05
    pub(crate) async fn changed(&self, workspace_id: WorkspaceId) {
        let appended = self
            .journal
            .append_event(
                EventKind::WorkspaceChanged.as_str(),
                serde_json::json!({ "workspace_id": workspace_id }),
            )
            .await;
        if appended.is_err() {
            log::workspace_event_failed(workspace_id);
        }
    }

    pub(crate) async fn workspace(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        store::load_workspace(&self.pool, id).await
    }

    /// A workspace that is not in the Bin (`WS_NOT_FOUND`, `WS_NOT_ACTIVE`).
    pub(crate) async fn not_in_bin(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        let workspace = self.workspace(id).await?;
        if workspace.state == WorkspaceState::InBin {
            return Err(WorkspaceError::NotActive {
                state: workspace.state,
            });
        }
        Ok(workspace)
    }

    async fn workspaces_not_in_bin(&self) -> Result<Vec<Workspace>, WorkspaceError> {
        self.list(WorkspaceListFilter::NotInBin).await
    }

    pub(crate) async fn place_ids_of(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<PlaceId>, WorkspaceError> {
        let rows = sqlx::query("SELECT place_id FROM workspace_places WHERE workspace_id = ?1 ORDER BY added_at, place_id")
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| store::id_of(r, "place_id")).collect()
    }

    /// The grants of a workspace as the File Service takes them.
    pub(crate) async fn grants_of(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<FolderGrant>, WorkspaceError> {
        let rows = sqlx::query(
            "SELECT * FROM folder_grants WHERE workspace_id = ?1 ORDER BY granted_at, id",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(crate::grants::grant_row).collect()
    }

    /// Hands the stored grants of one workspace to enforcement (Internal Logic 8.1).
    pub(crate) async fn publish(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let grants = self.grants_of(id).await?;
        self.files
            .publish_grants(id, grants)
            .await
            .map_err(|error| {
                tracing::warn!(
                    component = crate::log::COMPONENT,
                    code = error.code(),
                    "grant_publish_refused"
                );
                WorkspaceError::GrantPublishFailed
            })
    }

    async fn publish_with_backoff(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let mut last = self.publish(id).await;
        for wait in [1u64, 2, 4] {
            if last.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
            last = self.publish(id).await;
        }
        last
    }

    pub(crate) fn mark_unpublished(&self, id: WorkspaceId) {
        lock(&self.unpublished).insert(id);
    }

    async fn retry_unpublished(&self) {
        let pending: Vec<WorkspaceId> = lock(&self.unpublished).iter().copied().collect();
        for id in pending {
            let state = self.workspace(id).await.map(|w| w.state);
            let outcome = match state {
                Ok(WorkspaceState::InBin) => self
                    .files
                    .publish_grants(id, Vec::new())
                    .await
                    .map_err(|_| ()),
                Ok(_) => self.publish(id).await.map_err(|_| ()),
                Err(_) => Ok(()),
            };
            if outcome.is_ok() {
                lock(&self.unpublished).remove(&id);
            } else {
                log::grants_publish_failed(id);
            }
        }
    }

    // ---- 2. Creating and changing workspaces

    /// Trims and validates a workspace name (`WS_NAME_EMPTY`, `WS_NAME_TOO_LONG`).
    pub(crate) fn check_name(name: &str) -> Result<String, WorkspaceError> {
        let trimmed = name.trim();
        let chars = trimmed.chars().count();
        if chars == 0 {
            return Err(WorkspaceError::Invalid("WS_NAME_EMPTY"));
        }
        if chars > 80 {
            return Err(WorkspaceError::Invalid("WS_NAME_TOO_LONG"));
        }
        Ok(trimmed.to_owned())
    }

    /// `WS_NAME_DUPLICATE` when another workspace not in the Bin has the name, compared
    /// case-insensitively (*assumed here*).
    async fn check_name_free(
        &self,
        name: &str,
        except: Option<WorkspaceId>,
    ) -> Result<(), WorkspaceError> {
        let row = sqlx::query(
            "SELECT count(*) AS n FROM workspaces WHERE name_key = ?1 AND state != 'in_bin' AND id != ?2",
        )
        .bind(name.to_lowercase())
        .bind(except.map(|id| id.to_string()).unwrap_or_default())
        .fetch_one(&self.pool)
        .await?;
        if row.try_get::<i64, _>("n")? > 0 {
            return Err(WorkspaceError::Invalid("WS_NAME_DUPLICATE"));
        }
        Ok(())
    }

    pub async fn create(&self, name: String) -> Result<Workspace, WorkspaceError> {
        // 2.1
        let name = Self::check_name(&name)?;
        self.check_name_free(&name, None).await?;
        // 2.2
        let (folder, created_here) = self.make_own_folder(&name).await?;
        if self.rules.is_protected_location(&folder) {
            let _ = std::fs::remove_dir(&folder);
            return Err(WorkspaceError::OwnFolderFailed);
        }
        let place = match self
            .files
            .add_place(AddPlace {
                path: folder.clone(),
                friendly_name: Some(name.clone()),
                kind: PlaceKind::WorkspaceFolder,
            })
            .await
        {
            Ok(place) => place.id,
            Err(FileError::PlaceExists { existing }) => existing,
            Err(error) => {
                tracing::warn!(
                    component = crate::log::COMPONENT,
                    code = error.code(),
                    "own_folder_failed"
                );
                if created_here {
                    let _ = std::fs::remove_dir(&folder);
                }
                return Err(WorkspaceError::OwnFolderFailed);
            }
        };
        // 2.3
        let id = WorkspaceId::new();
        let now = Timestamp::now();
        let inserted = self.insert_workspace(id, &name, place, now).await;
        if let Err(error) = inserted {
            let _ = self.files.remove_place(place).await;
            return Err(error);
        }
        if let Err(error) = self.files.watch(&[place]).await {
            log::background_failed("watch", error.code());
        }
        self.record(
            "workspace_created",
            Some(id),
            "A workspace was created.",
            serde_json::json!({ "workspace_id": id }),
        )
        .await;
        log::workspace_event("workspace_created", id);
        self.changed(id).await;
        self.workspace(id).await
    }

    async fn insert_workspace(
        &self,
        id: WorkspaceId,
        name: &str,
        place: PlaceId,
        now: Timestamp,
    ) -> Result<(), WorkspaceError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO workspaces (id, name, name_key, own_folder_place_id, state, moved_to_bin_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, 'active', NULL, ?5)",
        )
        .bind(id.to_string())
        .bind(name)
        .bind(name.to_lowercase())
        .bind(place.to_string())
        .bind(now.to_wire())
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            if crate::error::is_unique_violation(&error) {
                WorkspaceError::Invalid("WS_NAME_DUPLICATE")
            } else {
                error.into()
            }
        })?;
        sqlx::query(
            "INSERT INTO workspace_places (workspace_id, place_id, added_at) VALUES (?1, ?2, ?3)",
        )
        .bind(id.to_string())
        .bind(place.to_string())
        .bind(now.to_wire())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Step 2.2: `OWN_FOLDER_ROOT/<name>`, with the characters `/ \ : * ? " < > |` replaced by `-`,
    /// numbered " (2)", " (3)" … while the folder exists and is not empty. Returns the folder and
    /// whether this call created it.
    async fn make_own_folder(&self, name: &str) -> Result<(PathBuf, bool), WorkspaceError> {
        let root = self.documents_dir.join("ordin");
        let clean: String = name
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
            .collect();
        tokio::task::spawn_blocking(move || {
            let mut candidate = root.join(&clean);
            let mut number = 2;
            loop {
                // WS-WC-1: the linked copies' folder is never a workspace's own folder.
                if candidate
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(crate::LINKED_COPIES_DIR))
                {
                    candidate = root.join(format!("{clean} ({number})"));
                    number += 1;
                    continue;
                }
                match std::fs::read_dir(&candidate) {
                    Ok(mut entries) => {
                        if entries.next().is_none() {
                            return Ok((candidate, false));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        std::fs::create_dir_all(&candidate)
                            .map_err(|_| WorkspaceError::OwnFolderFailed)?;
                        return Ok((candidate, true));
                    }
                    Err(_) => return Err(WorkspaceError::OwnFolderFailed),
                }
                candidate = root.join(format!("{clean} ({number})"));
                number += 1;
            }
        })
        .await
        .map_err(|_| WorkspaceError::OwnFolderFailed)?
    }

    pub async fn rename(&self, id: WorkspaceId, name: String) -> Result<Workspace, WorkspaceError> {
        let name = Self::check_name(&name)?;
        self.workspace(id).await?;
        self.check_name_free(&name, Some(id)).await?;
        sqlx::query("UPDATE workspaces SET name = ?2, name_key = ?3 WHERE id = ?1")
            .bind(id.to_string())
            .bind(&name)
            .bind(name.to_lowercase())
            .execute(&self.pool)
            .await?;
        self.record(
            "workspace_renamed",
            Some(id),
            "A workspace was renamed.",
            serde_json::json!({ "workspace_id": id }),
        )
        .await;
        log::workspace_event("workspace_renamed", id);
        self.changed(id).await;
        self.workspace(id).await
    }

    async fn transition(
        &self,
        id: WorkspaceId,
        from: WorkspaceState,
        to: WorkspaceState,
        kind: &'static str,
        summary: &'static str,
    ) -> Result<Workspace, WorkspaceError> {
        let workspace = self.workspace(id).await?;
        if workspace.state != from {
            return Err(WorkspaceError::StateInvalid);
        }
        sqlx::query("UPDATE workspaces SET state = ?2 WHERE id = ?1")
            .bind(id.to_string())
            .bind(to_text(&to))
            .execute(&self.pool)
            .await?;
        self.record(
            kind,
            Some(id),
            summary,
            serde_json::json!({ "workspace_id": id }),
        )
        .await;
        log::workspace_event(kind, id);
        self.changed(id).await;
        self.workspace(id).await
    }

    /// 2.5: active → archived. Grants stay published while archived (*assumed here*).
    pub async fn archive(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        self.transition(
            id,
            WorkspaceState::Active,
            WorkspaceState::Archived,
            "workspace_archived",
            "A workspace was archived.",
        )
        .await
    }

    /// 2.5: archived → active.
    pub async fn restore(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        self.transition(
            id,
            WorkspaceState::Archived,
            WorkspaceState::Active,
            "workspace_restored",
            "A workspace was restored.",
        )
        .await
    }

    /// 2.9
    pub async fn bin_impact(&self, id: WorkspaceId) -> Result<BinImpact, WorkspaceError> {
        self.workspace(id).await?;
        let rows = sqlx::query(
            "SELECT i.origin_path AS origin_path FROM working_copy_items i \
             JOIN source_links l ON l.id = i.source_link_id \
             WHERE l.workspace_id = ?1 AND i.state IN ('changed','awaiting_push','origin_changed') \
             ORDER BY i.origin_path",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut documents: Vec<String> = rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>("origin_path").ok())
            .map(|p| file_name(&p))
            .collect();
        let mut unsent = documents.len();
        let places = self.place_ids_of(id).await?;
        let held = self.files.held_edits(&places).await.map_err(file_failed)?;
        unsent += held.len();
        documents.extend(held.iter().map(|h| file_name(&h.path)));
        documents.truncate(10);
        Ok(BinImpact {
            unsent_changes: u32::try_from(unsent).unwrap_or(u32::MAX),
            documents,
        })
    }

    /// 2.6: active or archived → in the Bin. No agent reaches its places afterwards.
    pub async fn move_to_bin(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        let workspace = self.workspace(id).await?;
        if workspace.state == WorkspaceState::InBin {
            return Err(WorkspaceError::StateInvalid);
        }
        self.files
            .publish_grants(id, Vec::new())
            .await
            .map_err(|_| WorkspaceError::GrantPublishFailed)?;
        let moved = sqlx::query(
            "UPDATE workspaces SET state = 'in_bin', moved_to_bin_at = ?2 WHERE id = ?1",
        )
        .bind(id.to_string())
        .bind(Timestamp::now().to_wire())
        .execute(&self.pool)
        .await;
        if let Err(error) = moved {
            self.mark_unpublished(id);
            return Err(error.into());
        }
        // Unwatch the places used by no other workspace outside the Bin.
        let mut unused = Vec::new();
        for place in self.place_ids_of(id).await? {
            if !self.place_in_other_non_bin(place, id).await? {
                unused.push(place);
            }
        }
        self.files.unwatch(&unused).await;
        // Stop its timers: the debounced Confluence syncs.
        for link in self.confluence_link_ids(id).await? {
            lock(&self.sync_changes).remove(&link);
        }
        self.record(
            "workspace_moved_to_bin",
            Some(id),
            "A workspace was moved to the Bin.",
            serde_json::json!({ "workspace_id": id }),
        )
        .await;
        log::workspace_event("workspace_moved_to_bin", id);
        self.changed(id).await;
        self.workspace(id).await
    }

    async fn place_in_other_non_bin(
        &self,
        place: PlaceId,
        except: WorkspaceId,
    ) -> Result<bool, WorkspaceError> {
        let row = sqlx::query(
            "SELECT count(*) AS n FROM workspace_places p JOIN workspaces w ON w.id = p.workspace_id \
             WHERE p.place_id = ?1 AND p.workspace_id != ?2 AND w.state != 'in_bin'",
        )
        .bind(place.to_string())
        .bind(except.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get::<i64, _>("n")? > 0)
    }

    /// 2.7: in the Bin → active; the name must still be free.
    pub async fn restore_from_bin(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        let workspace = self.workspace(id).await?;
        if workspace.state != WorkspaceState::InBin {
            return Err(WorkspaceError::StateInvalid);
        }
        self.check_name_free(&workspace.name, Some(id)).await?;
        self.publish(id).await?;
        sqlx::query("UPDATE workspaces SET state = 'active', moved_to_bin_at = NULL WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|error| {
                if crate::error::is_unique_violation(&error) {
                    WorkspaceError::Invalid("WS_NAME_DUPLICATE")
                } else {
                    error.into()
                }
            })?;
        let places = self.place_ids_of(id).await?;
        if let Err(error) = self.files.watch(&places).await {
            log::background_failed("watch", error.code());
        }
        self.record(
            "workspace_restored_from_bin",
            Some(id),
            "A workspace was restored from the Bin.",
            serde_json::json!({ "workspace_id": id }),
        )
        .await;
        log::workspace_event("workspace_restored_from_bin", id);
        self.changed(id).await;
        if let Some(service) = self.me.upgrade() {
            self.spawn(async move {
                let _ = service.check_workspace(id).await;
            });
        }
        self.workspace(id).await
    }

    /// 2.8: removes ordin's records and its own working-copy folders only.
    pub async fn delete_from_bin(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let workspace = self.workspace(id).await?;
        if workspace.state != WorkspaceState::InBin {
            return Err(WorkspaceError::StateInvalid);
        }
        let links = self.links_of(id).await?;
        let mut removed_places: HashSet<PlaceId> = HashSet::new();
        for link in &links {
            if !is_remote(link.kind) {
                continue;
            }
            self.untrack(link).await;
            if let Some(place) = link.place_id {
                if self.files.remove_place(place).await.is_err() {
                    log::background_failed("remove_place", "WS_FILES_FAILED");
                }
                removed_places.insert(place);
            }
            self.remove_working_copy_dir(link.id).await;
        }
        if let Some(confluence) = &self.confluence {
            for link in self.confluence_link_ids(id).await? {
                if let Err(error) = confluence.forget_link(link).await {
                    log::background_failed("forget_link", ordin_types::CodedError::code(&error));
                }
            }
        }
        for place in self.place_ids_of(id).await? {
            if removed_places.contains(&place) {
                continue;
            }
            if !self.place_in_other_workspace(place, id).await?
                && self.files.remove_place(place).await.is_err()
            {
                log::background_failed("remove_place", "WS_FILES_FAILED");
            }
        }
        self.delete_rows(id).await?;
        for link in &links {
            lock(&self.progress).remove(&link.id);
        }
        self.record(
            "workspace_deleted",
            Some(id),
            "A workspace was deleted from the Bin.",
            serde_json::json!({ "workspace_id": id }),
        )
        .await;
        log::workspace_event("workspace_deleted", id);
        self.forget(id).await;
        let _ = self.deleted_tx.send(id);
        self.changed(id).await;
        Ok(())
    }

    /// `journal.forget_workspace`, journalling `workspace_forgotten` on success (2.8).
    async fn forget(&self, id: WorkspaceId) {
        match self.journal.forget_workspace(id).await {
            Ok(_) => {
                self.record(
                    "workspace_forgotten",
                    Some(id),
                    "The reports and dashboards of a deleted workspace were removed.",
                    serde_json::json!({ "workspace_id": id }),
                )
                .await;
            }
            Err(_) => log::workspace_forget_failed(id),
        }
    }

    /// 2.8: at start, the workspaces journalled `workspace_deleted` without `workspace_forgotten`.
    pub(crate) async fn retry_forgotten(&self) {
        let deleted = self.journalled_workspaces("workspace_deleted").await;
        let forgotten = self.journalled_workspaces("workspace_forgotten").await;
        for id in deleted.difference(&forgotten) {
            self.forget(*id).await;
        }
    }

    async fn journalled_workspaces(&self, kind: &str) -> HashSet<WorkspaceId> {
        let mut found = HashSet::new();
        let mut cursor = None;
        loop {
            let page = self
                .journal
                .list_entries(
                    ordin_journal::EntryFilter {
                        run_id: None,
                        kind: Some(kind.to_owned()),
                    },
                    cursor,
                )
                .await;
            let Ok(page) = page else { break };
            found.extend(page.items.iter().filter_map(|e| e.workspace_id));
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        found
    }

    async fn place_in_other_workspace(
        &self,
        place: PlaceId,
        except: WorkspaceId,
    ) -> Result<bool, WorkspaceError> {
        let row = sqlx::query(
            "SELECT count(*) AS n FROM workspace_places WHERE place_id = ?1 AND workspace_id != ?2",
        )
        .bind(place.to_string())
        .bind(except.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get::<i64, _>("n")? > 0)
    }

    /// The statements of 2.8 in one transaction, after the external calls.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-8
    async fn delete_rows(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let ws = id.to_string();
        let mut tx = self.pool.begin().await?;
        for sql in [
            "DELETE FROM working_copy_unread WHERE source_link_id IN (SELECT id FROM source_links WHERE workspace_id = ?1)",
            "DELETE FROM working_copy_items WHERE source_link_id IN (SELECT id FROM source_links WHERE workspace_id = ?1)",
            "DELETE FROM source_links WHERE workspace_id = ?1",
            "DELETE FROM confluence_links WHERE workspace_id = ?1",
            "DELETE FROM folder_grants WHERE workspace_id = ?1",
            "DELETE FROM email_links WHERE workspace_id = ?1",
            "DELETE FROM email_grants WHERE workspace_id = ?1",
            "DELETE FROM telegram_grants WHERE workspace_id = ?1",
            "DELETE FROM slack_grants WHERE workspace_id = ?1",
            // WS-TW-8: the team workspace's rows go in the same transaction.
            "DELETE FROM team_results_folders WHERE workspace_id = ?1",
            "DELETE FROM team_result_files WHERE workspace_id = ?1",
            "DELETE FROM team_pending_grants WHERE workspace_id = ?1",
            "DELETE FROM workspace_places WHERE workspace_id = ?1",
            "DELETE FROM workspaces WHERE id = ?1",
        ] {
            sqlx::query(sql).bind(&ws).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 2.8: repeats `delete_from_bin` for every workspace in the Bin and returns the count.
    pub async fn empty_bin(&self) -> Result<u32, WorkspaceError> {
        let binned = self.list(WorkspaceListFilter::InBin).await?;
        let mut count = 0;
        for workspace in binned {
            self.delete_from_bin(workspace.id).await?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        self.workspace(id).await
    }

    pub async fn list(
        &self,
        filter: WorkspaceListFilter,
    ) -> Result<Vec<Workspace>, WorkspaceError> {
        let sql = match filter {
            WorkspaceListFilter::Active => {
                "SELECT * FROM workspaces WHERE state = 'active' ORDER BY created_at, id"
            }
            WorkspaceListFilter::Archived => {
                "SELECT * FROM workspaces WHERE state = 'archived' ORDER BY created_at, id"
            }
            WorkspaceListFilter::InBin => {
                "SELECT * FROM workspaces WHERE state = 'in_bin' ORDER BY created_at, id"
            }
            WorkspaceListFilter::NotInBin => {
                "SELECT * FROM workspaces WHERE state != 'in_bin' ORDER BY created_at, id"
            }
        };
        let rows = sqlx::query(sql).fetch_all(&self.pool).await?;
        rows.iter().map(store::workspace_row).collect()
    }

    /// 2.10: a task, crew run or conversation may start only in an active workspace.
    pub async fn ensure_active(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        let workspace = self.workspace(id).await?;
        if workspace.state != WorkspaceState::Active {
            return Err(WorkspaceError::NotActive {
                state: workspace.state,
            });
        }
        Ok(workspace)
    }

    /// Runs the workspace's links' origin check for the person, waits up to 10 s for it and lets it
    /// finish in the background (Internal Logic 7.1).
    pub async fn open(&self, id: WorkspaceId) -> Result<crate::types::OpenReport, WorkspaceError> {
        let workspace = self.workspace(id).await?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        if let Some(service) = self.me.upgrade() {
            self.spawn(async move {
                let _ = sender.send(service.check_workspace(id).await);
            });
        }
        let (origin_changes, checks_failed) =
            match tokio::time::timeout(crate::OPEN_CHECK_WAIT, receiver).await {
                Ok(Ok(outcome)) => outcome,
                _ => (0, Vec::new()),
            };
        let _ = self.opened_tx.send(id);
        Ok(crate::types::OpenReport {
            workspace,
            origin_changes,
            checks_failed,
        })
    }

    /// The person closed the workspace: its debounced Confluence syncs stop.
    pub async fn close(&self, id: WorkspaceId) {
        if let Ok(links) = self.confluence_link_ids(id).await {
            let mut changes = lock(&self.sync_changes);
            for link in links {
                changes.remove(&link);
            }
        }
    }

    // ---- notices for later modules

    /// The id of each item that moves to `changed` (Internal Logic 6.2). Capacity 1,024.
    pub fn subscribe_working_copy_changes(&self) -> broadcast::Receiver<WorkingCopyItemId> {
        self.wc_tx.subscribe()
    }

    /// Capacity 1,024; a lagging receiver re-reads through `origin_version`.
    pub fn subscribe_origin_changes(&self) -> broadcast::Receiver<OriginChanged> {
        self.origin_tx.subscribe()
    }

    /// The id of each workspace the person opens (`open`). Capacity 256.
    pub fn opened(&self) -> broadcast::Receiver<WorkspaceId> {
        self.opened_tx.subscribe()
    }

    /// The id of each workspace deleted from the Bin (`delete_from_bin`, `empty_bin`). Capacity 256.
    pub fn deleted(&self) -> broadcast::Receiver<WorkspaceId> {
        self.deleted_tx.subscribe()
    }

    /// Each link `remove_link` removed, as (workspace, link), sent after the row is deleted.
    /// Capacity 256 (amended January 2026, ADR-103).
    pub fn links_removed(&self) -> broadcast::Receiver<(WorkspaceId, SourceLinkId)> {
        self.removed_tx.subscribe()
    }

    pub(crate) async fn confluence_link_ids(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<ConfluenceLinkId>, WorkspaceError> {
        let rows = sqlx::query("SELECT id FROM confluence_links WHERE workspace_id = ?1")
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| store::id_of(r, "id")).collect()
    }
}

/// A std mutex whose guard is never held across an await; a poisoned lock is still usable.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The last path segment of a stored path, as a person would read it.
pub(crate) fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_owned()
}

pub(crate) fn is_remote(kind: ordin_types::domain::places::SourceLinkKind) -> bool {
    use ordin_types::domain::places::SourceLinkKind as K;
    matches!(
        kind,
        K::SharepointLocation | K::ConfluenceLocation | K::JiraIssue
    )
}
