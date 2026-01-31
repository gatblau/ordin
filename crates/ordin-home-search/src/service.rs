//! `HomeSearch`: Home, *Needs you*, changed marks, search and **Run again** (HomeAndSearch.md
//! *Internal Logic* 1–5 and the ADR-106 amendment).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use futures::FutureExt;
use ordin_confluence::ConfluenceAdapter;
use ordin_files::{FileError, FileService, PlaceChange, PlaceChangeKind};
use ordin_journal::{Journal, ModuleMigrations};
use ordin_knowledge::KnowledgeReader;
use ordin_sharepoint::SharePointAdapter;
use ordin_types::common::InputUsed;
use ordin_types::domain::places::{ChangedMark, SourceLink, SourceLinkKind, Workspace};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::ContentRef;
use ordin_types::{KnowledgePageId, PlaceId, SourceLinkId, WorkspaceId};
use ordin_workspaces::{WorkspaceListFilter, WorkspaceService};
use sqlx::SqlitePool;
use tokio::sync::broadcast::error::RecvError;

use crate::error::{HomeError, PATH_INVALID};
use crate::ports::{FilesPort, Ports, RemoteChange, RemotePort, VersionRead, WorkspacesPort};
use crate::store;
use crate::types::{
    CopyUpdateSummary, FoundResult, GoToKind, GoToResult, HomeRow, HomeSourceError, HomeView,
    JournalReader, OpenTarget, RunAgainInput, RunAgainSubject, RunSummary, SearchAnswer,
    SearchSource, VersionCheck, WaitingItem,
};
use crate::COMPONENT;

/// 1 s (A-21); sources not answered by then are dropped from this answer.
pub(crate) const FIRST_RESULTS_DEADLINE: Duration = Duration::from_secs(1);
/// 8 results per group.
pub(crate) const GROUP_LIMIT: u32 = 8;
/// 200 rows in *Since you last looked*.
pub(crate) const SINCE_LIMIT: u32 = 200;
/// 60 s for the adapters' `changes_since`.
pub(crate) const REMOTE_MARK_POLL: Duration = Duration::from_secs(60);
/// 200 characters.
pub(crate) const QUERY_MAX: usize = 200;
/// A change to a path the person opened this recently is ignored (Internal Logic 3.1).
const OPENED_GRACE: Duration = Duration::from_secs(10);
/// *Since you last looked* when Home was never opened (Internal Logic 2.1).
const FIRST_LOOK_DAYS: i64 = 7;
/// `knowledge.search_pages` takes at most 500 workspaces per call.
const KNOWLEDGE_CHUNK: usize = 500;
/// A mark path is at most 4,096 bytes (2C rule).
const MARK_PATH_MAX_BYTES: usize = 4096;
/// Changed documents of one workspace and origin collapse into one row from this count.
const CHANGED_GROUP_MIN: usize = 3;
/// `SearchSource`s and `files.search` receive twice the group limit for documents (Internal Logic 4.2).
const FILE_HITS: u32 = 2 * GROUP_LIMIT;

/// The Actions group: (label, screen name) (11 §5.17).
// SPEC-GAP-2: result type and screen names settle with the Screens Module (HomeAndSearch.md Gaps).
const ACTIONS: &[(&str, &str)] = &[
    ("Home", "home"),
    ("New workspace", "new_workspace"),
    ("Add documents", "add_documents"),
    ("Summarise", "summarise"),
    ("Clear", "clear"),
    ("Earlier conversations", "earlier_conversations"),
    ("Settings", "settings"),
];

pub struct HomeSearchDeps {
    /// The datastore is `journal.datastore()`.
    // spec: ordin-desktop/components/HomeAndSearch#SA-RECON-01
    pub journal: Arc<Journal>,
    /// The program's composite.
    pub reader: Arc<dyn JournalReader>,
    /// `KnowledgeService::reader()`.
    pub knowledge: Arc<dyn KnowledgeReader>,
    pub files: Arc<FileService>,
    pub workspaces: Arc<WorkspaceService>,
    pub sharepoint: Arc<SharePointAdapter>,
    /// `None` in the personal server.
    pub confluence: Option<Arc<ConfluenceAdapter>>,
}

pub struct HomeSearch {
    pool: SqlitePool,
    ports: Ports,
    sources: RwLock<Vec<Arc<dyn SearchSource>>>,
    /// (place, path) → when `document_opened` last cleared it.
    opened: Mutex<HashMap<(PlaceId, String), Instant>>,
    started: AtomicBool,
    me: Weak<HomeSearch>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A relative path inside a place: 1–4,096 bytes, no leading `/` or `\`, no `..` or empty segment.
pub(crate) fn valid_mark_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MARK_PATH_MAX_BYTES
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains('\0')
        && path.split('/').all(|s| !s.is_empty() && s != "..")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 0 exact (case-insensitive), 1 prefix, 2 anything else (Internal Logic 4.5).
fn rank(label: &str, query: &str) -> u8 {
    let label = label.to_lowercase();
    let query = query.to_lowercase();
    if label == query {
        0
    } else if label.starts_with(&query) {
        1
    } else {
        2
    }
}

fn contains_text(label: &str, query: &str) -> bool {
    label.to_lowercase().contains(&query.to_lowercase())
}

fn source_label(kind: SourceLinkKind) -> Option<&'static str> {
    match kind {
        SourceLinkKind::SharepointLocation => Some("SharePoint"),
        SourceLinkKind::ConfluenceLocation => Some("Confluence"),
        SourceLinkKind::JiraIssue => Some("Jira"),
        _ => None,
    }
}

/// The result of one search branch.
enum Branch {
    Workspaces(Vec<Workspace>),
    GoTo(Vec<GoToResult>),
    Knowledge(Vec<GoToResult>, Vec<FoundResult>),
    Files(Vec<GoToResult>, Vec<FoundResult>, Option<(u64, u64)>),
}

impl HomeSearch {
    /// `ModuleMigrations { module: "home_search", sqlite: &[(1, "home_search tables", …)], postgres: &[] }`;
    /// the program registers it with the Journal.
    // spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-04
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: "home_search",
            sqlite: &[(1, "home_search tables", include_str!("sql/sqlite_v1.sql"))],
            postgres: &[],
        }
    }

    pub async fn new(deps: HomeSearchDeps) -> Result<Arc<HomeSearch>, HomeError> {
        let Some(pool) = deps.journal.datastore().sqlite().cloned() else {
            // Desktop and personal server only: SQLite.
            return Err(HomeError::DatastoreUnavailable);
        };
        let confluence = deps
            .confluence
            .map(|c| -> Arc<dyn RemotePort> { c as Arc<dyn RemotePort> });
        let ports = Ports {
            reader: deps.reader,
            knowledge: deps.knowledge,
            files: deps.files as Arc<dyn FilesPort>,
            workspaces: deps.workspaces as Arc<dyn WorkspacesPort>,
            sharepoint: deps.sharepoint as Arc<dyn RemotePort>,
            confluence,
        };
        Ok(Self::with_ports(pool, ports))
    }

    pub(crate) fn with_ports(pool: SqlitePool, ports: Ports) -> Arc<HomeSearch> {
        Arc::new_cyclic(|me| HomeSearch {
            pool,
            ports,
            sources: RwLock::new(Vec::new()),
            opened: Mutex::new(HashMap::new()),
            started: AtomicBool::new(false),
            me: me.clone(),
        })
    }

    pub fn register_source(&self, source: Arc<dyn SearchSource>) {
        self.sources
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .push(source);
    }

    /// Internal Logic 1: the File Service listener and the remote mark poll. A second call does
    /// nothing. Both tasks end when the module is dropped.
    pub async fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut changes = self.ports.files.subscribe_changes();
        let me = self.me.clone();
        tokio::spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(change) => {
                        let Some(this) = me.upgrade() else { break };
                        this.on_place_change(&change).await;
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        // The File Service offers no replay; remote marks come from the polls.
                        tracing::warn!(component = COMPONENT, skipped, "marks_lagged");
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
        let me = self.me.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(REMOTE_MARK_POLL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let Some(this) = me.upgrade() else { break };
                this.poll_remote_marks().await;
            }
        });
    }

    // ---------------------------------------------------------------- 2. Home

    /// Workspace names of every workspace not in the Bin; `None` when the list cannot be read.
    async fn names(&self) -> Option<HashMap<WorkspaceId, String>> {
        match self
            .ports
            .workspaces
            .list(WorkspaceListFilter::NotInBin)
            .await
        {
            Ok(list) => Some(list.into_iter().map(|w| (w.id, w.name)).collect()),
            Err(_) => {
                tracing::warn!(
                    component = COMPONENT,
                    source = "workspaces",
                    code = "HOME_SOURCE_UNAVAILABLE",
                    "home_source_unavailable"
                );
                None
            }
        }
    }

    fn unavailable(source: &'static str) {
        tracing::warn!(
            component = COMPONENT,
            source,
            code = "HOME_SOURCE_UNAVAILABLE",
            "home_source_unavailable"
        );
    }

    /// Internal Logic 2.2: "Waiting for you", grouped by workspace (groups by their oldest item),
    /// oldest first, then the items of no workspace under "Hosted agents".
    fn waiting_rows(
        items: Vec<WaitingItem>,
        names: Option<&HashMap<WorkspaceId, String>>,
        filter: Option<WorkspaceId>,
        last_opened: Option<Timestamp>,
    ) -> Vec<HomeRow> {
        let mut groups: BTreeMap<Option<WorkspaceId>, Vec<WaitingItem>> = BTreeMap::new();
        for item in items {
            if !keep(item.workspace_id, names, filter) {
                continue;
            }
            groups.entry(item.workspace_id).or_default().push(item);
        }
        let hosted = groups.remove(&None);
        let mut ordered: Vec<Vec<WaitingItem>> = groups.into_values().collect();
        for group in ordered.iter_mut() {
            group.sort_by_key(|i| i.since);
        }
        ordered.sort_by_key(|g| g.first().map(|i| i.since));
        // spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-05
        if let Some(mut group) = hosted {
            group.sort_by_key(|i| i.since);
            ordered.push(group);
        }
        ordered
            .into_iter()
            .flatten()
            .map(|item| HomeRow {
                workspace_id: item.workspace_id,
                workspace_name: name_of(names, item.workspace_id),
                line: item.title,
                at: item.since,
                is_new: is_new(item.since, last_opened),
                opens: item.opens,
                run_again: false,
            })
            .collect()
    }

    pub async fn home(&self, filter: Option<WorkspaceId>) -> Result<HomeView, HomeError> {
        let started = Instant::now();
        let last_opened = store::home_last_opened(&self.pool).await?;
        let now = Timestamp::now();
        let since = last_opened
            .unwrap_or_else(|| Timestamp(now.0 - chrono::Duration::days(FIRST_LOOK_DAYS)));
        let p = &self.ports;
        let (waiting, running, finished, origin, pages, marks, names) = tokio::join!(
            p.reader.waiting_items(),
            p.reader.running(),
            p.reader.finished_since(since, SINCE_LIMIT),
            p.workspaces.origin_changes_since(since),
            p.knowledge.out_of_date_since(since),
            store::marks_since(&self.pool, since),
            self.names(),
        );
        if let (Some(f), Some(n)) = (filter, names.as_ref()) {
            if !n.contains_key(&f) {
                return Err(HomeError::WorkspaceNotFound);
            }
        }
        let names = names.as_ref();
        let row = |ws: WorkspaceId, line: String, at: Timestamp, opens: OpenTarget, again: bool| {
            HomeRow {
                workspace_id: Some(ws),
                workspace_name: name_of(names, Some(ws)),
                line,
                at,
                is_new: is_new(at, last_opened),
                opens,
                run_again: again,
            }
        };

        let waiting = match waiting {
            Ok(items) => Self::waiting_rows(items, names, filter, last_opened),
            Err(_) => {
                Self::unavailable("waiting");
                Vec::new()
            }
        };

        let mut running_rows = Vec::new();
        match running {
            Ok(list) => {
                for run in list {
                    if !keep(Some(run.workspace_id), names, filter) {
                        continue;
                    }
                    let Some(opens) = run_target(&run) else {
                        continue;
                    };
                    running_rows.push(row(
                        run.workspace_id,
                        format!("{} — {}", run.title, run.state),
                        run.at,
                        opens,
                        false,
                    ));
                }
            }
            Err(_) => Self::unavailable("running"),
        }

        let mut since_rows = Vec::new();
        match finished {
            Ok(list) => {
                for run in list {
                    if !keep(Some(run.workspace_id), names, filter) {
                        continue;
                    }
                    let Some(opens) = run_target(&run) else {
                        continue;
                    };
                    // spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-02
                    let line = if run.state == "replied_in_slack" {
                        format!("{} replied in Slack", run.title)
                    } else {
                        run.title.clone()
                    };
                    since_rows.push(row(
                        run.workspace_id,
                        line,
                        run.at,
                        opens,
                        run.can_run_again,
                    ));
                }
            }
            Err(_) => Self::unavailable("finished"),
        }

        // Links of every workspace whose marks or copy updates are shown, for names and origins.
        let marks = match marks {
            Ok(m) => m,
            Err(e) => {
                Self::unavailable("marks");
                tracing::debug!(component = COMPONENT, code = %e, "home.marks_unread");
                Vec::new()
            }
        };
        let origin = match origin {
            Ok(o) => o,
            Err(_) => {
                Self::unavailable("origin_changes");
                Vec::new()
            }
        };
        let mut wanted: Vec<WorkspaceId> = marks.iter().map(|m| m.workspace_id).collect();
        wanted.extend(origin.iter().map(|o| o.workspace_id));
        wanted.sort();
        wanted.dedup();
        let mut links: HashMap<WorkspaceId, Vec<SourceLink>> = HashMap::new();
        for ws in wanted {
            if !keep(Some(ws), names, filter) {
                continue;
            }
            match p.workspaces.source_links(ws).await {
                Ok(l) => {
                    links.insert(ws, l);
                }
                Err(_) => Self::unavailable("source_links"),
            }
        }

        // Changed documents: three or more in one workspace and origin collapse into one row.
        let mut by_origin: BTreeMap<(WorkspaceId, Option<&'static str>), Vec<ChangedMark>> =
            BTreeMap::new();
        for mark in marks {
            if !keep(Some(mark.workspace_id), names, filter) {
                continue;
            }
            let origin = links.get(&mark.workspace_id).and_then(|ls| {
                ls.iter()
                    .find(|l| l.place_id == Some(mark.place_id))
                    .and_then(|l| source_label(l.kind))
            });
            by_origin
                .entry((mark.workspace_id, origin))
                .or_default()
                .push(mark);
        }
        for ((ws, origin), group) in by_origin {
            match origin {
                Some(label) if group.len() >= CHANGED_GROUP_MIN => {
                    let at = group.iter().map(|m| m.changed_at).max().unwrap_or(since);
                    since_rows.push(row(
                        ws,
                        format!("{} documents changed in {label}", group.len()),
                        at,
                        OpenTarget::Workspace { workspace_id: ws },
                        false,
                    ));
                }
                _ => {
                    for mark in group {
                        since_rows.push(row(
                            ws,
                            format!("{} changed", file_name(&mark.path)),
                            mark.changed_at,
                            OpenTarget::Document {
                                workspace_id: ws,
                                place_id: mark.place_id,
                                path: mark.path.clone(),
                            },
                            false,
                        ));
                    }
                }
            }
        }

        match pages {
            Ok(list) => {
                for page in list {
                    if !keep(Some(page.workspace_id), names, filter) {
                        continue;
                    }
                    // spec: ordin-desktop/components/HomeAndSearch#SA-RECON-03
                    since_rows.push(row(
                        page.workspace_id,
                        format!("{} may be out of date", page.title),
                        page.last_changed_at,
                        OpenTarget::KnowledgePage {
                            workspace_id: page.workspace_id,
                            page_id: page.id,
                        },
                        false,
                    ));
                }
            }
            Err(_) => Self::unavailable("knowledge"),
        }

        for update in copy_updates(&origin, &links, since) {
            if !keep(Some(update.workspace_id), names, filter) {
                continue;
            }
            let noun = if update.count == 1 {
                "document"
            } else {
                "documents"
            };
            since_rows.push(row(
                update.workspace_id,
                format!(
                    "{} {noun} in {} updated from {}",
                    update.count, update.original_name, update.source
                ),
                update.at,
                OpenTarget::Workspace {
                    workspace_id: update.workspace_id,
                },
                false,
            ));
        }

        since_rows.sort_by_key(|r| std::cmp::Reverse(r.at));
        since_rows.truncate(SINCE_LIMIT as usize);
        let new_count = since_rows.iter().filter(|r| r.is_new).count();
        let view = HomeView {
            waiting,
            running: running_rows,
            new_count: u32::try_from(new_count).unwrap_or(u32::MAX),
            since_last_looked: since_rows,
        };
        tracing::info!(
            component = COMPONENT,
            rows = view.waiting.len() + view.running.len() + view.since_last_looked.len(),
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "home_built"
        );
        Ok(view)
    }

    /// The *Needs you* rows alone, for the Needs you tab and the desktop notifier.
    pub async fn needs_you(&self, filter: Option<WorkspaceId>) -> Result<Vec<HomeRow>, HomeError> {
        let last_opened = store::home_last_opened(&self.pool).await?;
        let (items, names) = tokio::join!(self.ports.reader.waiting_items(), self.names());
        if let (Some(f), Some(n)) = (filter, names.as_ref()) {
            if !n.contains_key(&f) {
                return Err(HomeError::WorkspaceNotFound);
            }
        }
        let items = items.map_err(|_| {
            Self::unavailable("waiting");
            HomeError::SourceUnavailable
        })?;
        Ok(Self::waiting_rows(
            items,
            names.as_ref(),
            filter,
            last_opened,
        ))
    }

    pub async fn home_opened(&self) -> Result<(), HomeError> {
        store::set_home_opened(&self.pool, Timestamp::now()).await
    }

    /// Writes nothing into the conversation (ADR-079).
    pub async fn workspace_opened(&self, id: WorkspaceId) -> Result<(), HomeError> {
        store::set_last_seen(&self.pool, id, Timestamp::now()).await
    }

    /// Fills `Workspace.last_seen_at` (B7 contract D-9).
    pub async fn with_last_seen(&self, ws: Vec<Workspace>) -> Result<Vec<Workspace>, HomeError> {
        let seen = store::last_seen_all(&self.pool).await?;
        Ok(ws
            .into_iter()
            .map(|mut w| {
                w.last_seen_at = seen.get(&w.id).copied();
                w
            })
            .collect())
    }

    // ---------------------------------------------------------------- 3. Changed marks

    pub async fn changed_marks(
        &self,
        workspace: WorkspaceId,
    ) -> Result<Vec<ChangedMark>, HomeError> {
        store::marks_of(&self.pool, workspace).await
    }

    /// Clears the changed mark of the item; `ContentRef::Artefact` has no mark and returns `Ok`
    /// (reconciliation row 184). A kept file version (`FileVersion`, a 2C variant the spec's copy
    /// predates) names no current path either and returns `Ok`.
    pub async fn document_opened(
        &self,
        workspace: WorkspaceId,
        item: ContentRef,
    ) -> Result<(), HomeError> {
        let ContentRef::PlaceFile {
            place_id,
            relative_path,
        } = item
        else {
            return Ok(());
        };
        if !valid_mark_path(&relative_path) {
            return Err(HomeError::Invalid(PATH_INVALID));
        }
        lock(&self.opened).insert((place_id, relative_path.clone()), Instant::now());
        let removed =
            store::delete_mark(&self.pool, Some(workspace), place_id, &relative_path).await?;
        if removed > 0 {
            tracing::info!(component = COMPONENT, added = 0, removed, "marks_updated");
        }
        Ok(())
    }

    fn recently_opened(&self, place: PlaceId, path: &str) -> bool {
        let mut opened = lock(&self.opened);
        opened.retain(|_, at| at.elapsed() < OPENED_GRACE);
        opened.contains_key(&(place, path.to_owned()))
    }

    /// Internal Logic 3.1: a change made outside ordin marks the document in every non-bin
    /// workspace holding the place.
    pub(crate) async fn on_place_change(&self, change: &PlaceChange) {
        if change.by_ordin {
            return;
        }
        let workspaces = match self
            .ports
            .workspaces
            .list(WorkspaceListFilter::NotInBin)
            .await
        {
            Ok(list) => list,
            Err(_) => {
                Self::unavailable("workspaces");
                return;
            }
        };
        let mut holders = Vec::new();
        for ws in workspaces {
            match self.ports.workspaces.places(ws.id).await {
                Ok(places) if places.iter().any(|p| p.id == change.place_id) => holders.push(ws.id),
                Ok(_) => {}
                Err(_) => Self::unavailable("places"),
            }
        }
        let (mut added, mut removed) = (0u64, 0u64);
        let result: Result<(), HomeError> = async {
            match &change.change {
                PlaceChangeKind::Removed => {
                    for ws in &holders {
                        removed += store::delete_mark(
                            &self.pool,
                            Some(*ws),
                            change.place_id,
                            &change.path,
                        )
                        .await?;
                    }
                }
                kind => {
                    if let PlaceChangeKind::Renamed { from } = kind {
                        for ws in &holders {
                            removed +=
                                store::delete_mark(&self.pool, Some(*ws), change.place_id, from)
                                    .await?;
                        }
                    }
                    if !valid_mark_path(&change.path)
                        || self.recently_opened(change.place_id, &change.path)
                    {
                        return Ok(());
                    }
                    for ws in &holders {
                        if store::upsert_mark(
                            &self.pool,
                            *ws,
                            change.place_id,
                            &change.path,
                            change.observed_at,
                        )
                        .await?
                        {
                            added += 1;
                        }
                    }
                }
            }
            Ok(())
        }
        .await;
        if result.is_ok() && added + removed > 0 {
            tracing::info!(component = COMPONENT, added, removed, "marks_updated");
        }
    }

    /// Internal Logic 3.2 (SharePoint and Confluence), 3.2a (Jira, ADR-106) and 3.5 (workspaces no
    /// longer listed). `remote_mark_poll` holds one time for every adapter, so it moves on only
    /// when every source answered: a failed adapter's changes are read again at the next poll,
    /// and the upsert absorbs what the others repeat.
    pub(crate) async fn poll_remote_marks(&self) {
        let now = Timestamp::now();
        let since = match store::last_poll(&self.pool).await {
            Ok(Some(at)) => at,
            Ok(None) => {
                Timestamp(now.0 - chrono::Duration::from_std(REMOTE_MARK_POLL).unwrap_or_default())
            }
            Err(_) => return,
        };
        let p = &self.ports;
        let mut all_answered = true;
        let mut links: HashMap<SourceLinkId, SourceLink> = HashMap::new();
        match p.workspaces.list(WorkspaceListFilter::Active).await {
            Ok(active) => {
                for ws in active {
                    match p.workspaces.source_links(ws.id).await {
                        Ok(list) => links.extend(list.into_iter().map(|l| (l.id, l))),
                        Err(_) => {
                            Self::unavailable("source_links");
                            all_answered = false;
                        }
                    }
                }
            }
            Err(_) => {
                Self::unavailable("workspaces");
                return;
            }
        }
        let ids_of = |kind: SourceLinkKind| -> Vec<SourceLinkId> {
            let mut ids: Vec<SourceLinkId> = links
                .values()
                .filter(|l| l.kind == kind)
                .map(|l| l.id)
                .collect();
            ids.sort();
            ids
        };
        let mut changes: Vec<RemoteChange> = Vec::new();
        let sharepoint_ids = ids_of(SourceLinkKind::SharepointLocation);
        if !sharepoint_ids.is_empty() {
            match p.sharepoint.changes_since(&sharepoint_ids, since).await {
                Ok(c) => changes.extend(c),
                Err(_) => {
                    Self::unavailable("sharepoint");
                    all_answered = false;
                }
            }
        }
        let confluence_ids = ids_of(SourceLinkKind::ConfluenceLocation);
        if let Some(confluence) = &p.confluence {
            if !confluence_ids.is_empty() {
                match confluence.changes_since(&confluence_ids, since).await {
                    Ok(c) => changes.extend(c),
                    Err(_) => {
                        Self::unavailable("confluence");
                        all_answered = false;
                    }
                }
            }
        }
        // ADR-106: Jira items through the Workspace Service; no Jira call is made here.
        match p.workspaces.origin_changes_since(since).await {
            Ok(list) => {
                for c in list
                    .into_iter()
                    .filter(|c| c.link_kind == SourceLinkKind::JiraIssue)
                {
                    if !links.contains_key(&c.source_link_id) {
                        if let Ok(more) = p.workspaces.source_links(c.workspace_id).await {
                            links.extend(more.into_iter().map(|l| (l.id, l)));
                        }
                    }
                    changes.push(RemoteChange {
                        source_link_id: c.source_link_id,
                        origin_path: c.origin_path,
                        changed_at: c.version.changed_at.unwrap_or(now),
                        removed: c.removed,
                    });
                }
            }
            Err(_) => {
                Self::unavailable("origin_changes");
                all_answered = false;
            }
        }
        let (mut added, mut removed) = (0u64, 0u64);
        for change in changes {
            // spec: ordin-desktop/components/HomeAndSearch#SA-RECON-04
            let Some(link) = links.get(&change.source_link_id) else {
                continue;
            };
            let Some(place) = link.place_id else {
                continue;
            };
            if !valid_mark_path(&change.origin_path) {
                continue;
            }
            let done = if change.removed {
                store::delete_mark(
                    &self.pool,
                    Some(link.workspace_id),
                    place,
                    &change.origin_path,
                )
                .await
                .map(|n| removed += n)
            } else {
                store::upsert_mark(
                    &self.pool,
                    link.workspace_id,
                    place,
                    &change.origin_path,
                    change.changed_at,
                )
                .await
                .map(|new| added += u64::from(new))
            };
            if done.is_err() {
                all_answered = false;
            }
        }
        // 3.5: marks of workspaces no longer listed at all.
        let (not_in_bin, in_bin) = tokio::join!(
            p.workspaces.list(WorkspaceListFilter::NotInBin),
            p.workspaces.list(WorkspaceListFilter::InBin)
        );
        if let (Ok(a), Ok(b)) = (not_in_bin, in_bin) {
            let listed: HashSet<WorkspaceId> = a.iter().chain(b.iter()).map(|w| w.id).collect();
            match store::forget_unlisted(&self.pool, &listed).await {
                Ok(n) => removed += n,
                Err(_) => all_answered = false,
            }
        }
        if all_answered && store::set_last_poll(&self.pool, now).await.is_err() {
            return;
        }
        if added + removed > 0 {
            tracing::info!(component = COMPONENT, added, removed, "marks_updated");
        }
    }

    // ---------------------------------------------------------------- 4. Search

    pub async fn search(
        &self,
        query: &str,
        current: Option<WorkspaceId>,
    ) -> Result<SearchAnswer, HomeError> {
        let started = Instant::now();
        let query = query.trim();
        let chars = query.chars().count();
        if chars == 0 || chars > QUERY_MAX {
            return Err(HomeError::QueryInvalid);
        }
        let sources: Vec<Arc<dyn SearchSource>> = self
            .sources
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let mut branches: Vec<BoxFuture<'_, Branch>> = vec![
            self.search_workspaces().boxed(),
            self.search_knowledge(query).boxed(),
            self.search_files(query, current).boxed(),
        ];
        for source in sources {
            let q = query.to_owned();
            branches.push(
                async move {
                    let group = source.group();
                    let mut found = source.find(&q, GROUP_LIMIT).await;
                    for r in found.iter_mut() {
                        r.kind = group;
                    }
                    Branch::GoTo(found)
                }
                .boxed(),
            );
        }
        let answered = futures::future::join_all(
            branches
                .into_iter()
                .map(|b| tokio::time::timeout(FIRST_RESULTS_DEADLINE, b)),
        )
        .await;
        let mut complete = true;
        let mut names: Option<HashMap<WorkspaceId, String>> = None;
        let mut go_to: Vec<GoToResult> = Vec::new();
        let mut found: Vec<FoundResult> = Vec::new();
        let mut coverage = None;
        for result in answered {
            match result {
                Err(_) => complete = false,
                Ok(Branch::Workspaces(list)) => {
                    for w in &list {
                        if contains_text(&w.name, query) {
                            go_to.push(GoToResult {
                                kind: GoToKind::Workspace,
                                label: w.name.clone(),
                                detail: None,
                                workspace_id: Some(w.id),
                                opens: OpenTarget::Workspace { workspace_id: w.id },
                            });
                        }
                    }
                    names = Some(list.into_iter().map(|w| (w.id, w.name)).collect());
                }
                Ok(Branch::GoTo(r)) => go_to.extend(r),
                Ok(Branch::Knowledge(g, f)) => {
                    go_to.extend(g);
                    found.extend(f);
                }
                Ok(Branch::Files(g, f, c)) => {
                    go_to.extend(g);
                    found.extend(f);
                    coverage = c;
                }
            }
        }
        // 4.4: a result from another workspace names it.
        if let Some(names) = &names {
            for r in go_to.iter_mut() {
                if r.kind == GoToKind::Workspace {
                    continue;
                }
                if let Some(ws) = r.workspace_id {
                    if Some(ws) != current {
                        if let Some(name) = names.get(&ws) {
                            r.detail = Some(format!("· {name}"));
                        }
                    }
                }
            }
        }
        // 4.5: per group, exact, prefix, then source order; at most GROUP_LIMIT.
        let mut grouped: Vec<GoToResult> = Vec::new();
        for kind in [
            GoToKind::Workspace,
            GoToKind::Agent,
            GoToKind::Workflow,
            GoToKind::Dashboard,
            GoToKind::KnowledgePage,
            GoToKind::Conversation,
            GoToKind::Document,
        ] {
            let mut group: Vec<GoToResult> =
                go_to.iter().filter(|r| r.kind == kind).cloned().collect();
            group.sort_by_key(|r| rank(&r.label, query));
            group.truncate(GROUP_LIMIT as usize);
            grouped.extend(group);
        }
        found.sort_by_key(|r| rank(&r.label, query));
        found.truncate(GROUP_LIMIT as usize);
        let actions = actions(query);
        let answer = SearchAnswer {
            query: query.to_owned(),
            go_to: grouped,
            found,
            actions,
            coverage,
            complete,
        };
        tracing::info!(
            component = COMPONENT,
            groups = 3,
            results = answer.go_to.len() + answer.found.len() + answer.actions.len(),
            complete,
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "search_answered"
        );
        Ok(answer)
    }

    async fn search_workspaces(&self) -> Branch {
        match self
            .ports
            .workspaces
            .list(WorkspaceListFilter::NotInBin)
            .await
        {
            Ok(list) => Branch::Workspaces(list),
            Err(_) => {
                Self::unavailable("workspaces");
                Branch::GoTo(Vec::new())
            }
        }
    }

    async fn search_knowledge(&self, query: &str) -> Branch {
        let ids: Vec<WorkspaceId> = match self
            .ports
            .workspaces
            .list(WorkspaceListFilter::NotInBin)
            .await
        {
            Ok(list) => list.into_iter().map(|w| w.id).collect(),
            Err(_) => {
                Self::unavailable("workspaces");
                return Branch::Knowledge(Vec::new(), Vec::new());
            }
        };
        let mut go_to = Vec::new();
        let mut found = Vec::new();
        for chunk in ids.chunks(KNOWLEDGE_CHUNK) {
            match self
                .ports
                .knowledge
                .search_pages(chunk, query, GROUP_LIMIT)
                .await
            {
                Ok(hits) => {
                    for hit in hits {
                        let opens = OpenTarget::KnowledgePage {
                            workspace_id: hit.workspace_id,
                            page_id: hit.id,
                        };
                        if contains_text(&hit.title, query) {
                            go_to.push(GoToResult {
                                kind: GoToKind::KnowledgePage,
                                label: hit.title,
                                detail: None,
                                workspace_id: Some(hit.workspace_id),
                                opens,
                            });
                        } else {
                            found.push(FoundResult {
                                label: hit.title,
                                snippet: Some(hit.snippet),
                                workspace_id: hit.workspace_id,
                                opens,
                            });
                        }
                    }
                }
                Err(_) => Self::unavailable("knowledge"),
            }
        }
        Branch::Knowledge(go_to, found)
    }

    async fn search_files(&self, query: &str, current: Option<WorkspaceId>) -> Branch {
        let empty = || Branch::Files(Vec::new(), Vec::new(), None);
        let list = match self
            .ports
            .workspaces
            .list(WorkspaceListFilter::NotInBin)
            .await
        {
            Ok(list) => list,
            Err(_) => {
                Self::unavailable("workspaces");
                return empty();
            }
        };
        // A place in several workspaces is shown in the current one when it holds it.
        let mut owner: HashMap<PlaceId, WorkspaceId> = HashMap::new();
        for ws in &list {
            match self.ports.workspaces.places(ws.id).await {
                Ok(places) => {
                    for place in places {
                        let entry = owner.entry(place.id).or_insert(ws.id);
                        if Some(ws.id) == current {
                            *entry = ws.id;
                        }
                    }
                }
                Err(_) => Self::unavailable("places"),
            }
        }
        let mut ids: Vec<PlaceId> = owner.keys().copied().collect();
        ids.sort();
        let results = match self.ports.files.search(&ids, query, FILE_HITS).await {
            Ok(r) => r,
            Err(_) => {
                Self::unavailable("files");
                return empty();
            }
        };
        let mut go_to = Vec::new();
        let mut found = Vec::new();
        for hit in results.hits {
            let Some(ws) = owner.get(&hit.place_id).copied() else {
                continue;
            };
            let opens = OpenTarget::Document {
                workspace_id: ws,
                place_id: hit.place_id,
                path: hit.path.clone(),
            };
            let label = file_name(&hit.path).to_owned();
            if hit.name_match {
                go_to.push(GoToResult {
                    kind: GoToKind::Document,
                    label,
                    detail: None,
                    workspace_id: Some(ws),
                    opens,
                });
            } else {
                found.push(FoundResult {
                    label,
                    snippet: hit.snippet,
                    workspace_id: ws,
                    opens,
                });
            }
        }
        let c = results.coverage;
        let coverage = (c.indexed < c.total).then_some((c.indexed, c.total));
        Branch::Files(go_to, found, coverage)
    }

    // ---------------------------------------------------------------- 5. Run again

    pub async fn run_again_inputs(
        &self,
        subject: RunAgainSubject,
    ) -> Result<Vec<RunAgainInput>, HomeError> {
        let inputs = self
            .ports
            .reader
            .run_inputs(subject)
            .await
            .map_err(|e| match e {
                HomeSourceError::NotFound => HomeError::RunNotFound,
                HomeSourceError::Unavailable => HomeError::SourceUnavailable,
            })?;
        let mut out = Vec::with_capacity(inputs.len());
        for input in inputs {
            let current = self.current_version(&input).await;
            let (check, newer_version) = match current {
                Current::Version(v) if v == input.version_read => (VersionCheck::Same, None),
                Current::Version(v) => (VersionCheck::Newer, Some(v)),
                Current::Gone => (VersionCheck::Gone, None),
                Current::CannotCheck => (VersionCheck::CannotCheck, None),
            };
            out.push(RunAgainInput {
                input,
                newer_version,
                check,
            });
        }
        Ok(out)
    }

    async fn current_version(&self, input: &InputUsed) -> Current {
        let location = input.location.as_str();
        if let Some(rest) = location.strip_prefix("place:") {
            let Some((place, path)) = rest.split_once('/') else {
                return Current::CannotCheck;
            };
            let Ok(place) = place.parse::<PlaceId>() else {
                return Current::CannotCheck;
            };
            return match self.ports.files.file_hash(place, path).await {
                Ok(hash) => Current::Version(hash),
                Err(FileError::NotFound | FileError::PlaceNotFound) => Current::Gone,
                Err(_) => Current::CannotCheck,
            };
        }
        if let Some(rest) = location.strip_prefix("graph:") {
            return remote_version(self.ports.sharepoint.version(rest).await);
        }
        if let Some(rest) = location.strip_prefix("confluence:") {
            return match &self.ports.confluence {
                Some(c) => remote_version(c.version(rest).await),
                None => Current::CannotCheck,
            };
        }
        if let Some(rest) = location.strip_prefix("knowledge:") {
            let Ok(page) = rest.parse::<KnowledgePageId>() else {
                return Current::CannotCheck;
            };
            return match self.ports.knowledge.page_version(page).await {
                Ok(Some(v)) => Current::Version(v),
                Ok(None) => Current::Gone,
                Err(_) => Current::CannotCheck,
            };
        }
        // Git commits, email, Telegram: a repository folder is a place, so git originals are
        // compared through `place:`; a commit-addressed input would need the Project Service.
        Current::CannotCheck
    }
}

enum Current {
    Version(String),
    Gone,
    CannotCheck,
}

fn remote_version(read: VersionRead) -> Current {
    match read {
        VersionRead::Tag(tag) => Current::Version(tag),
        VersionRead::NotFound => Current::Gone,
        VersionRead::Failed => Current::CannotCheck,
    }
}

fn is_new(at: Timestamp, last_opened: Option<Timestamp>) -> bool {
    last_opened.is_none_or(|last| at > last)
}

fn name_of(
    names: Option<&HashMap<WorkspaceId, String>>,
    ws: Option<WorkspaceId>,
) -> Option<String> {
    ws.and_then(|id| names.and_then(|n| n.get(&id).cloned()))
}

/// The filter keeps one workspace's rows (and leaves out the items of no workspace); workspaces in
/// the Bin are left out when the list could be read.
fn keep(
    ws: Option<WorkspaceId>,
    names: Option<&HashMap<WorkspaceId, String>>,
    filter: Option<WorkspaceId>,
) -> bool {
    if let Some(f) = filter {
        if ws != Some(f) {
            return false;
        }
    }
    match (ws, names) {
        (Some(id), Some(n)) => n.contains_key(&id),
        _ => true,
    }
}

fn run_target(run: &RunSummary) -> Option<OpenTarget> {
    if let Some(run_id) = run.run_id {
        return Some(OpenTarget::Run {
            workspace_id: run.workspace_id,
            run_id,
        });
    }
    run.crew_run_id.map(|crew_run_id| OpenTarget::CrewRun {
        workspace_id: run.workspace_id,
        crew_run_id,
    })
}

/// Copy updates grouped by workspace and link, Jira left out (ADR-106 item 2), at most
/// `SINCE_LIMIT` groups.
// spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-03
pub(crate) fn copy_updates(
    changes: &[ordin_workspaces::OriginChanged],
    links: &HashMap<WorkspaceId, Vec<SourceLink>>,
    since: Timestamp,
) -> Vec<CopyUpdateSummary> {
    let mut groups: BTreeMap<(WorkspaceId, SourceLinkId), CopyUpdateSummary> = BTreeMap::new();
    for change in changes {
        let source = match change.link_kind {
            SourceLinkKind::SharepointLocation => "SharePoint",
            SourceLinkKind::ConfluenceLocation => "Confluence",
            _ => continue,
        };
        let Some(link) = links
            .get(&change.workspace_id)
            .and_then(|ls| ls.iter().find(|l| l.id == change.source_link_id))
        else {
            continue;
        };
        let at = change.version.changed_at.unwrap_or(since);
        let entry = groups
            .entry((change.workspace_id, change.source_link_id))
            .or_insert_with(|| CopyUpdateSummary {
                workspace_id: change.workspace_id,
                original_name: link.display_name.clone(),
                count: 0,
                source: source.to_owned(),
                at,
            });
        entry.count += 1;
        if at > entry.at {
            entry.at = at;
        }
    }
    let mut out: Vec<CopyUpdateSummary> = groups.into_values().collect();
    out.sort_by_key(|u| std::cmp::Reverse(u.at));
    out.truncate(SINCE_LIMIT as usize);
    out
}

/// The fixed Actions list, filtered by case-insensitive prefix or word match.
fn actions(query: &str) -> Vec<GoToResult> {
    let q = query.to_lowercase();
    ACTIONS
        .iter()
        .filter(|(label, _)| {
            let l = label.to_lowercase();
            l.starts_with(&q) || l.split_whitespace().any(|w| w.starts_with(&q))
        })
        .map(|(label, screen)| GoToResult {
            kind: GoToKind::Workspace,
            label: (*label).to_owned(),
            detail: None,
            workspace_id: None,
            opens: OpenTarget::Screen {
                name: (*screen).to_owned(),
            },
        })
        .collect()
}
