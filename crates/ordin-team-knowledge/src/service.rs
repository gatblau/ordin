//! The service object: construction, `run` (§A), the per-workspace lock, the long commands (§H),
//! status, turning on and off (§B, §C), and the Git and export helpers the other sections share.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use git2::Oid;
use ordin_journal::{ModuleMigrations, NewJournalEntry};
use ordin_projects::{GithubError, RepoPermission};
use ordin_types::domain::{
    SourceLinkKind, SourceLinkState, TeamAccess, TeamFolderRole, TeamKnowledgeBinding,
    WorkspaceState,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::events::EventKind;
use ordin_types::{RunId, TeamFolderId, WorkspaceId};
use ordin_workspaces::{LinkLocation, WorkspaceError};
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::TeamError;
use crate::files::{self, ItemKind};
use crate::git::{self, GitFailure};
use crate::model::{TeamBusy, TeamFolderChoice, TeamKnowledgeStatus};
use crate::ports::TeamKnowledgeDeps;
use crate::store::{self, Binding};

/// Module name for the Journal's `MigrationRegistry` (ruling R3).
pub const MODULE: &str = "team_knowledge";
/// Checks of a bound folder every 15 minutes while ordin runs (Configuration).
pub const CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// 120 s per fetch; 3 attempts on a network error, 2 s then 8 s apart (Configuration).
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(120);
pub const FETCH_ATTEMPTS: u32 = 3;
/// At most 3 rebuilds when the branch moves during a send (Configuration).
pub const MAX_MOVES: u32 = 3;
/// Soft folder size: 20,000 fact files (SA-TEAM-07).
pub const SOFT_FOLDER_FACTS: usize = 20_000;
/// A fact batch for `import_team` (§F step 4).
pub const IMPORT_BATCH: usize = 200;
/// The run hook's queue (TW-F step 1).
const RUN_QUEUE: usize = 1_024;
/// Access is re-read at most once an hour (§D step 1).
const ACCESS_RECHECK: Duration = Duration::from_secs(3_600);

/// What a test may change: the remote address, the login, and the waits (never in a program build).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Seams {
    /// `<base>/<owner>/<name>.git` instead of `https://<host>/<owner>/<name>.git`.
    pub remote_base: Option<PathBuf>,
    /// The login `git_credentials` would give (SA-TEAM-TW-02); `GitToken` has no public maker.
    pub login: Option<String>,
    pub no_waits: bool,
}

pub struct TeamKnowledgeService {
    pub(crate) deps: TeamKnowledgeDeps,
    pub(crate) pool: Option<SqlitePool>,
    pub(crate) me: Weak<TeamKnowledgeService>,
    locks: Mutex<HashMap<WorkspaceId, Arc<tokio::sync::Mutex<()>>>>,
    busy: Mutex<HashMap<WorkspaceId, TeamBusy>>,
    errors: Mutex<HashMap<WorkspaceId, &'static str>>,
    pub(crate) runs_tx: mpsc::Sender<RunId>,
    runs_rx: tokio::sync::Mutex<Option<mpsc::Receiver<RunId>>>,
    #[cfg(test)]
    pub(crate) seams: Mutex<Seams>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// `owner/name` and the host of `https://<host>/<owner>/<name>(.git)?` (or `git@<host>:…`).
pub(crate) fn split_url(url: &str) -> Option<(String, String)> {
    let rest = url
        .strip_prefix("https://")
        .map(|r| r.replacen('/', "\u{0}", 1))
        .or_else(|| {
            url.strip_prefix("git@")
                .map(|r| r.replacen(':', "\u{0}", 1))
        })?;
    let (host, path) = rest.split_once('\u{0}')?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let parts: Vec<&str> = path.split('/').collect();
    if host.is_empty()
        || host.contains('@')
        || parts.len() != 2
        || parts
            .iter()
            .any(|p| p.is_empty() || p.contains(char::is_whitespace))
    {
        return None;
    }
    Some((
        host.to_ascii_lowercase(),
        format!("{}/{}", parts[0], parts[1]),
    ))
}

/// `git:<url>#<branch>:<path>` → (host, repository, branch, folder).
pub(crate) fn parse_origin(origin: &str) -> Option<(String, String, String, String)> {
    let rest = origin.strip_prefix("git:")?;
    let (url, tail) = rest.split_once('#')?;
    let (branch, path) = tail.split_once(':')?;
    let (host, repository) = split_url(url)?;
    if branch.is_empty() || branch.len() > 255 || path.len() > 1_024 {
        return None;
    }
    if path.split('/').any(|s| s == "..") || path.starts_with('/') {
        return None;
    }
    Some((
        host,
        repository,
        branch.to_owned(),
        path.trim_end_matches('/').to_owned(),
    ))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// One item of this workspace as it would be written to the folder (§E step 4).
#[derive(Debug, Clone)]
pub(crate) struct LocalFile {
    pub kind: ItemKind,
    pub item_id: String,
    pub bytes: Vec<u8>,
    /// A page's title, for the card's "changed pages" line.
    pub title: Option<String>,
}

/// The workspace's exported knowledge, rendered.
#[derive(Debug, Default)]
pub(crate) struct Local {
    pub files: BTreeMap<String, LocalFile>,
    pub facts: HashMap<String, ordin_memory_service::TeamFact>,
    pub held_for_check: u32,
}

/// The folder at a fetched tip.
#[derive(Debug, Clone)]
pub(crate) struct Tip {
    pub commit: Oid,
    pub files: BTreeMap<String, Oid>,
}

impl TeamKnowledgeService {
    /// Module "team_knowledge", for the Journal's MigrationRegistry (ruling R3). Steps 2 and 3 are
    /// applied as their amendments write them, their tables empty in this build (Data Model of the
    /// team workspace amendment).
    // spec: ordin-desktop/components/TeamKnowledgeService#SA-TEAM-02
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: MODULE,
            sqlite: &[
                (1, "team knowledge", include_str!("sql/sqlite_v1.sql")),
                (2, "read folders", include_str!("sql/sqlite_v2.sql")),
                (
                    3,
                    "open knowledge format bundles",
                    include_str!("sql/sqlite_v3.sql"),
                ),
                (4, "team workspaces", include_str!("sql/sqlite_v4.sql")),
            ],
            postgres: &[],
        }
    }

    pub fn new(deps: TeamKnowledgeDeps) -> Arc<Self> {
        let pool = deps.journal.datastore().sqlite().cloned();
        let (runs_tx, runs_rx) = mpsc::channel(RUN_QUEUE);
        Arc::new_cyclic(|me| TeamKnowledgeService {
            deps,
            pool,
            me: me.clone(),
            locks: Mutex::new(HashMap::new()),
            busy: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
            runs_tx,
            runs_rx: tokio::sync::Mutex::new(Some(runs_rx)),
            #[cfg(test)]
            seams: Mutex::new(Seams::default()),
        })
    }

    pub(crate) fn pool(&self) -> Result<&SqlitePool, TeamError> {
        self.pool.as_ref().ok_or_else(TeamError::datastore)
    }

    pub(crate) fn workspace_lock(&self, ws: WorkspaceId) -> Arc<tokio::sync::Mutex<()>> {
        lock(&self.locks).entry(ws).or_default().clone()
    }

    pub(crate) fn set_busy(&self, ws: WorkspaceId, busy: TeamBusy) {
        lock(&self.busy).insert(ws, busy);
    }

    pub(crate) fn busy(&self, ws: WorkspaceId) -> Option<TeamBusy> {
        lock(&self.busy).get(&ws).copied()
    }

    pub(crate) async fn wait(&self, delay: Duration) {
        #[cfg(test)]
        if lock(&self.seams).no_waits {
            return;
        }
        tokio::time::sleep(delay).await;
    }

    // SPEC-GAP-8: Shared Context 3 of the team workspace amendment registers the run hook's sink "with
    // `Journal::subscribe` at `run`", while OrdinDesktop.md DSK-TW-2 has row 3.41a call
    // `journal.subscribe` after construction. The sink is registered here, once, and the program does
    // not register it again.
    // spec: ordin-desktop/components/TeamKnowledgeService#TW-F
    /// Start-up check of every binding, the 15-minute timer, the workspace and push listeners, and the
    /// run hook (§A, TW-F). Returns on `shutdown`.
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken) {
        let sink = crate::sink::RunSink::new(Arc::downgrade(&self));
        let subscription = self.deps.journal.subscribe(Arc::new(sink));
        let mut opened = self.deps.workspaces.opened();
        let mut deleted = self.deps.workspaces.deleted();
        let mut removed = self.deps.workspaces.links_removed();
        let mut notices = self.deps.push.subscribe();
        let mut runs = self.runs_rx.lock().await.take();

        // §A step 1.
        if let Ok(pool) = self.pool() {
            if let Ok(bindings) = store::bindings(pool).await {
                for binding in bindings {
                    if shutdown.is_cancelled() {
                        break;
                    }
                    self.check_logged(binding.workspace_id).await;
                }
            }
        }
        let mut timer = tokio::time::interval(CHECK_INTERVAL);
        timer.tick().await;
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = timer.tick() => {
                    if let Ok(pool) = self.pool() {
                        if let Ok(bindings) = store::bindings(pool).await {
                            for binding in bindings {
                                self.check_logged(binding.workspace_id).await;
                            }
                        }
                    }
                }
                got = opened.recv() => match got {
                    Ok(ws) => {
                        if self.pool().is_ok() {
                            if let Ok(Some(_)) = store::binding(self.pool().expect("pool"), ws).await {
                                self.check_logged(ws).await;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => opened = self.deps.workspaces.opened(),
                    Err(_) => {}
                },
                got = deleted.recv() => match got {
                    Ok(ws) => self.workspace_deleted(ws).await,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => deleted = self.deps.workspaces.deleted(),
                    Err(_) => {}
                },
                got = removed.recv() => match got {
                    Ok((ws, link)) => self.link_removed(ws, link).await,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => removed = self.deps.workspaces.links_removed(),
                    Err(_) => {}
                },
                got = notices.recv() => match got {
                    Ok(notice) => self.on_notice(notice).await,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // §A step 4: shares in `approved` or `sending` are settled by the next check.
                        tracing::warn!(target: "ordin::team_knowledge", "team.push_notices_lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => notices = self.deps.push.subscribe(),
                },
                got = async { match runs.as_mut() { Some(r) => r.recv().await, None => std::future::pending().await } } => {
                    match got {
                        Some(run_id) => self.record_run(run_id).await,
                        None => runs = None,
                    }
                }
            }
        }
        self.deps.journal.unsubscribe(subscription);
    }

    async fn check_logged(&self, ws: WorkspaceId) {
        let guard = self.workspace_lock(ws);
        let _held = guard.lock().await;
        if let Err(e) = self.check(ws).await {
            tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code, "team.check_failed");
            if let Ok(pool) = self.pool() {
                let _ = store::set_last_error(pool, ws, Some(e.code)).await;
            }
        }
    }

    async fn workspace_deleted(&self, ws: WorkspaceId) {
        if let Ok(pool) = self.pool() {
            let _ = store::delete_workspace(pool, ws).await;
        }
        let _ = std::fs::remove_dir_all(self.clone_path(ws));
    }

    async fn link_removed(&self, ws: WorkspaceId, link: ordin_types::SourceLinkId) {
        let Ok(pool) = self.pool() else {
            return;
        };
        // TW-I step 3: a place of the description; the next share removes it.
        let _ = sqlx::query("DELETE FROM team_workspace_places WHERE source_link_id = ?1")
            .bind(link.to_string())
            .execute(pool)
            .await;
        if let Ok(Some(binding)) = store::binding(pool, ws).await {
            if binding.source_link_id == link {
                let guard = self.workspace_lock(ws);
                let _held = guard.lock().await;
                if let Err(e) = self.turn_off_locked(ws, "link_removed").await {
                    tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code, "team.turn_off_failed");
                }
            }
        }
    }

    // ---- journal and events ----

    /// `team_knowledge.<name>` as kind `team_knowledge_<name>`, ids and counts only.
    pub(crate) async fn journal(
        &self,
        name: &str,
        ws: WorkspaceId,
        summary: &str,
        data: serde_json::Value,
    ) {
        let entry = NewJournalEntry {
            kind: format!("team_knowledge_{name}"),
            component: "team_knowledge".to_owned(),
            run_id: None,
            crew_run_id: None,
            workspace_id: Some(ws),
            summary: summary.to_owned(),
            data,
        };
        if let Err(e) = self.deps.journal.append_entry(entry).await {
            tracing::warn!(target: "ordin::team_knowledge", error = %e, "team.journal_failed");
        }
    }

    pub(crate) async fn changed(&self, ws: WorkspaceId) {
        let appended = self
            .deps
            .journal
            .append_event(
                EventKind::WorkspaceChanged.as_str(),
                serde_json::json!({ "workspace_id": ws }),
            )
            .await;
        if appended.is_err() {
            tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), "team.event_failed");
        }
    }

    // ---- Git ----

    pub(crate) fn clone_path(&self, ws: WorkspaceId) -> PathBuf {
        self.deps.clones_dir.join(format!("{ws}.git"))
    }

    pub(crate) fn remote_url(&self, host: &str, repository: &str) -> String {
        #[cfg(test)]
        if let Some(base) = &lock(&self.seams).remote_base {
            return base
                .join(format!("{repository}.git"))
                .to_string_lossy()
                .into_owned();
        }
        format!("https://{host}/{repository}.git")
    }

    /// §B step 4's fetch with its attempts and timeout, into the clone at `path`.
    pub(crate) async fn fetch(
        &self,
        path: PathBuf,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<Oid, TeamError> {
        let url = self.remote_url(host, repository);
        let mut attempt = 0;
        loop {
            attempt += 1;
            let repos = Arc::clone(&self.deps.repos);
            let handle = tokio::runtime::Handle::current();
            let (p, u, b, h) = (
                path.clone(),
                url.clone(),
                branch.to_owned(),
                host.to_owned(),
            );
            let task = tokio::task::spawn_blocking(move || {
                let credentials = || {
                    handle
                        .block_on(repos.git_credentials(&h))
                        .map(|c| (c.username, c.token.expose().to_owned()))
                        .map_err(|e| match e {
                            GithubError::Unreachable | GithubError::RateLimited { .. } => {
                                GitFailure::Unreachable
                            }
                            _ => GitFailure::SignIn,
                        })
                };
                git::fetch_once(&p, &u, &b, &credentials)
            });
            let outcome = match tokio::time::timeout(FETCH_TIMEOUT, task).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(GitFailure::Disk),
                Err(_) => Err(GitFailure::Unreachable),
            };
            match outcome {
                Ok(tip) => return Ok(tip),
                Err(GitFailure::Unreachable) if attempt < FETCH_ATTEMPTS => {
                    let delay = if attempt == 1 { 2 } else { 8 };
                    self.wait(Duration::from_secs(delay)).await;
                }
                Err(GitFailure::Unreachable) => {
                    return Err(TeamError::of("TEAM_REMOTE_UNREACHABLE"))
                }
                Err(GitFailure::SignIn) => return Err(TeamError::of("TEAM_SIGN_IN_NEEDED")),
                Err(GitFailure::NoBranch) => {
                    return Err(TeamError::permanent("TEAM_LINK_NOT_SUITABLE"))
                }
                Err(GitFailure::Disk) => return Err(TeamError::permanent("TEAM_CLONE_FAILED")),
            }
        }
    }

    /// The files below `folder` at `tip` in the clone at `path`.
    pub(crate) async fn tip_files(
        &self,
        path: PathBuf,
        tip: Oid,
        folder: &str,
    ) -> Result<Tip, TeamError> {
        let folder = folder.to_owned();
        let files = tokio::task::spawn_blocking(move || {
            let repo = git2::Repository::open_bare(&path).map_err(|_| GitFailure::Disk)?;
            git::folder_files(&repo, tip, &folder)
        })
        .await
        .map_err(|_| TeamError::permanent("TEAM_CLONE_FAILED"))?
        .map_err(|_| TeamError::permanent("TEAM_CLONE_FAILED"))?;
        Ok(Tip { commit: tip, files })
    }

    /// Blobs read from the clone, each bounded by `max`; a blob over the bound reads as `None`.
    pub(crate) async fn read_blobs(
        &self,
        path: PathBuf,
        wanted: Vec<(String, Oid, u64)>,
    ) -> Result<HashMap<String, Option<Vec<u8>>>, TeamError> {
        tokio::task::spawn_blocking(move || {
            let repo = git2::Repository::open_bare(&path).map_err(|_| GitFailure::Disk)?;
            let mut out = HashMap::with_capacity(wanted.len());
            for (key, oid, max) in wanted {
                out.insert(key, git::read_blob(&repo, oid, max)?);
            }
            Ok::<_, GitFailure>(out)
        })
        .await
        .map_err(|_| TeamError::permanent("TEAM_CLONE_FAILED"))?
        .map_err(|_| TeamError::permanent("TEAM_CLONE_FAILED"))
    }

    /// The fetched folder at its tip, with the format rule of §D step 3. `Err(TEAM_FORMAT_NEWER)`
    /// only when `refuse_newer`; otherwise the problem is recorded.
    pub(crate) async fn fetch_folder(&self, b: &Binding) -> Result<Tip, TeamError> {
        let path = self.clone_path(b.workspace_id);
        let tip = self
            .fetch(path.clone(), &b.host, &b.repository, &b.branch)
            .await?;
        self.tip_files(path, tip, &b.folder).await
    }

    /// The folder's format: `Ok(Some(n))`, `Ok(None)` absent, `Err(())` unreadable.
    pub(crate) async fn folder_format(
        &self,
        ws: WorkspaceId,
        tip: &Tip,
    ) -> Result<Result<Option<u64>, ()>, TeamError> {
        let Some(oid) = tip.files.get(files::FORMAT_FILE) else {
            return Ok(Ok(None));
        };
        let blobs = self
            .read_blobs(
                self.clone_path(ws),
                vec![(
                    files::FORMAT_FILE.to_owned(),
                    *oid,
                    files::ITEM_MAX_BYTES as u64,
                )],
            )
            .await?;
        let bytes = blobs.get(files::FORMAT_FILE).cloned().flatten();
        Ok(files::read_format(bytes.as_deref())
            .and_then(|f| f.ok_or(()))
            .map(Some))
    }

    // ---- export ----

    /// The person's name for a never-shared item (§E step 4); "Someone using ordin" when unknown.
    pub(crate) async fn person_name(&self) -> String {
        match self.deps.repos.person().await {
            Ok(p) if !p.name.trim().is_empty() => p.name,
            _ => "Someone using ordin".to_owned(),
        }
    }

    /// §E step 3 and 4: every fact, entity, source, relationship and term of the workspace's scope,
    /// and every page, as file bytes.
    pub(crate) async fn export_local(
        &self,
        ws: WorkspaceId,
        name: &str,
    ) -> Result<Local, TeamError> {
        let mut local = Local::default();
        let mut cursor = None;
        loop {
            let page = self
                .deps
                .memory
                .export_team(ws, cursor)
                .await
                .map_err(|e| TeamError::per_cause("TEAM_MEMORY_UNAVAILABLE", e.retryable()))?;
            local.held_for_check = local.held_for_check.max(page.held_for_check);
            for fact in page.facts {
                let path = ItemKind::Fact.path(&fact.id.to_string());
                local.files.insert(
                    path.clone(),
                    LocalFile {
                        kind: ItemKind::Fact,
                        item_id: fact.id.to_string(),
                        bytes: files::render_fact(&fact, name),
                        title: None,
                    },
                );
                local.facts.insert(path, fact);
            }
            for e in page.entities {
                if let Some(stem) = e.id.strip_prefix("entity:") {
                    local.files.insert(
                        ItemKind::Entity.path(stem),
                        LocalFile {
                            kind: ItemKind::Entity,
                            item_id: e.id.clone(),
                            bytes: files::render(&e),
                            title: None,
                        },
                    );
                }
            }
            for s in page.sources {
                if let Some(stem) = s.id.strip_prefix("source:") {
                    local.files.insert(
                        ItemKind::Source.path(stem),
                        LocalFile {
                            kind: ItemKind::Source,
                            item_id: s.id.clone(),
                            bytes: files::render(&s),
                            title: None,
                        },
                    );
                }
            }
            for r in page.relationships {
                if let Some(stem) = r.id.strip_prefix("relationship:") {
                    local.files.insert(
                        ItemKind::Relationship.path(stem),
                        LocalFile {
                            kind: ItemKind::Relationship,
                            item_id: r.id.clone(),
                            bytes: files::render(&r),
                            title: None,
                        },
                    );
                }
            }
            for t in page.terms {
                local.files.insert(
                    ItemKind::Term.path(&t.key),
                    LocalFile {
                        kind: ItemKind::Term,
                        item_id: t.key.clone(),
                        bytes: files::render(&t),
                        title: None,
                    },
                );
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        let pages = self
            .deps
            .pages
            .export_team_pages(ws)
            .await
            .map_err(|e| TeamError::per_cause("TEAM_PAGES_UNAVAILABLE", e.retryable()))?;
        for page in pages {
            let id = page.page_id.to_string();
            let bytes = files::name_page(&page.bytes, name);
            let title = std::str::from_utf8(&bytes)
                .ok()
                .and_then(|t| {
                    ordin_knowledge::okf::parse_concept(&format!("pages/{id}.md"), t).ok()
                })
                .map(|c| c.title);
            local.files.insert(
                ItemKind::Page.path(&id),
                LocalFile {
                    kind: ItemKind::Page,
                    item_id: id,
                    bytes,
                    title,
                },
            );
        }
        Ok(local)
    }

    // ---- status ----

    pub async fn status(&self, ws: WorkspaceId) -> Result<TeamKnowledgeStatus, TeamError> {
        let pool = self.pool()?;
        let binding = store::binding(pool, ws).await?;
        let busy = self.busy(ws);
        let last_error = lock(&self.errors).get(&ws).copied();
        let team_workspace = self.team_workspace_status(ws, binding.as_ref()).await?;
        let Some(b) = binding else {
            return Ok(TeamKnowledgeStatus {
                workspace_id: ws,
                binding: None,
                header_line: None,
                available: None,
                to_share: None,
                held_for_check: 0,
                waiting_for_review: None,
                problems: Vec::new(),
                share_unavailable: None,
                last_checked_at: None,
                folder_large: false,
                busy,
                last_get: None,
                last_share: None,
                last_error,
                team_workspace,
            });
        };
        let open = store::open_share(pool, ws).await?;
        let waiting_for_review = open
            .as_ref()
            .filter(|s| s.state == "waiting_review")
            .and_then(|s| s.pull_request.clone());
        let share_unavailable = if b.access == TeamAccess::ReadOnly {
            Some("TEAM_READ_ONLY")
        } else if open.is_some() {
            Some("TEAM_SHARE_PENDING")
        } else {
            None
        };
        let problems = store::problems(pool, ws)
            .await?
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        Ok(TeamKnowledgeStatus {
            workspace_id: ws,
            header_line: Some(b.header_line()),
            binding: Some(binding_view(&b)),
            available: b.available.clone().filter(|a| !a.counts.is_empty()),
            to_share: b.to_share,
            held_for_check: b.held_for_check,
            waiting_for_review,
            problems,
            share_unavailable,
            last_checked_at: b.last_checked_at,
            folder_large: b.folder_large,
            busy,
            last_get: b.last_get.clone(),
            last_share: b.last_share.clone(),
            last_error: last_error.or_else(|| b.last_error_code.as_deref().map(store::static_code)),
            team_workspace,
        })
    }

    /// The workspace's `GitRemote` links that are active (the choices of **Share knowledge with a
    /// team…**).
    pub async fn candidate_links(
        &self,
        ws: WorkspaceId,
    ) -> Result<Vec<ordin_types::domain::SourceLink>, TeamError> {
        self.active_workspace(ws).await?;
        let links = self
            .deps
            .workspaces
            .source_links(ws)
            .await
            .map_err(|_| TeamError::permanent("TEAM_WORKSPACE_NOT_FOUND"))?;
        Ok(links
            .into_iter()
            .filter(|l| l.kind == SourceLinkKind::GitRemote && l.state == SourceLinkState::Active)
            .collect())
    }

    pub(crate) async fn active_workspace(
        &self,
        ws: WorkspaceId,
    ) -> Result<ordin_types::domain::Workspace, TeamError> {
        match self.deps.workspaces.get(ws).await {
            Ok(w) if w.state != WorkspaceState::InBin => Ok(w),
            _ => Err(TeamError::permanent("TEAM_WORKSPACE_NOT_FOUND")),
        }
    }

    // ---- §B turning it on ----

    /// The folder a choice names: (link id, host, repository, branch, folder). Adds the link for a
    /// `NewFolder` only when `add` is set.
    pub(crate) async fn resolve_choice(
        &self,
        ws: WorkspaceId,
        choice: &TeamFolderChoice,
        add: bool,
    ) -> Result<
        (
            Option<ordin_types::SourceLinkId>,
            String,
            String,
            String,
            String,
        ),
        TeamError,
    > {
        let unsuitable = || TeamError::permanent("TEAM_LINK_NOT_SUITABLE");
        match choice {
            TeamFolderChoice::Link { source_link_id } => {
                let links = self
                    .deps
                    .workspaces
                    .source_links(ws)
                    .await
                    .map_err(|_| unsuitable())?;
                let link = links
                    .into_iter()
                    .find(|l| {
                        l.id == *source_link_id
                            && l.kind == SourceLinkKind::GitRemote
                            && l.state == SourceLinkState::Active
                    })
                    .ok_or_else(unsuitable)?;
                let (h, r, b, f) = parse_origin(&link.origin_location).ok_or_else(unsuitable)?;
                Ok((Some(link.id), h, r, b, f))
            }
            TeamFolderChoice::NewFolder { url, branch, path } => {
                let (host, repository) = split_url(url).ok_or_else(unsuitable)?;
                if branch.is_empty()
                    || branch.len() > 255
                    || path.len() > 1_024
                    || path.split('/').any(|s| s == "..")
                    || path.starts_with('/')
                {
                    return Err(unsuitable());
                }
                let folder = path.trim_end_matches('/').to_owned();
                if !add {
                    return Ok((None, host, repository, branch.clone(), folder));
                }
                let link = self
                    .deps
                    .workspaces
                    .add_link(
                        ws,
                        LinkLocation::GitRemote {
                            url: url.clone(),
                            branch: branch.clone(),
                            path: path.clone(),
                        },
                        None,
                    )
                    .await
                    .map_err(|e| match e {
                        WorkspaceError::NotFound | WorkspaceError::NotActive { .. } => {
                            TeamError::permanent("TEAM_WORKSPACE_NOT_FOUND")
                        }
                        _ => unsuitable(),
                    })?;
                Ok((Some(link.id), host, repository, branch.clone(), folder))
            }
        }
    }

    /// §B step 3's mapping.
    pub(crate) async fn access_of(
        &self,
        host: &str,
        repository: &str,
    ) -> Result<TeamAccess, TeamError> {
        match self
            .deps
            .repos
            .repository_permission(host, repository)
            .await
        {
            Ok(RepoPermission::Write | RepoPermission::Maintain | RepoPermission::Admin) => {
                Ok(TeamAccess::ReadWrite)
            }
            Ok(RepoPermission::Read | RepoPermission::Triage) => Ok(TeamAccess::ReadOnly),
            Ok(RepoPermission::None) | Err(GithubError::NotFound) => {
                Err(TeamError::permanent("TEAM_NO_ACCESS"))
            }
            Err(GithubError::SignInNeeded) => Err(TeamError::of("TEAM_SIGN_IN_NEEDED")),
            Err(GithubError::HostNotSupported) => {
                Err(TeamError::permanent("TEAM_LINK_NOT_SUITABLE"))
            }
            Err(GithubError::Unreachable | GithubError::RateLimited { .. }) => {
                Err(TeamError::of("TEAM_REMOTE_UNREACHABLE"))
            }
            Err(_) => Err(TeamError::of("TEAM_REMOTE_UNREACHABLE")),
        }
    }

    // spec: ordin-desktop/components/TeamKnowledgeService#SA-TEAM-01
    pub async fn turn_on(
        &self,
        ws: WorkspaceId,
        choice: TeamFolderChoice,
    ) -> Result<TeamKnowledgeStatus, TeamError> {
        let guard = self.workspace_lock(ws);
        let _held = guard.lock().await;
        self.turn_on_locked(ws, &choice).await?;
        drop(_held);
        self.status(ws).await
    }

    /// §B steps 1–6; the caller holds the workspace's lock.
    pub(crate) async fn turn_on_locked(
        &self,
        ws: WorkspaceId,
        choice: &TeamFolderChoice,
    ) -> Result<Binding, TeamError> {
        let pool = self.pool()?;
        self.active_workspace(ws).await?;
        if store::binding(pool, ws).await?.is_some() {
            return Err(TeamError::permanent("TEAM_ALREADY_ON"));
        }
        // Access is read before a link is added, so a refusal leaves the workspace as it was.
        if let TeamFolderChoice::NewFolder { .. } = choice {
            let (_, host, repository, _, _) = self.resolve_choice(ws, choice, false).await?;
            self.access_of(&host, &repository).await?;
        }
        let (link, host, repository, branch, folder) =
            self.resolve_choice(ws, choice, true).await?;
        let link = link.ok_or_else(|| TeamError::permanent("TEAM_LINK_NOT_SUITABLE"))?;
        let access = self.access_of(&host, &repository).await?;
        let path = self.clone_path(ws);
        let fetched = self.fetch(path.clone(), &host, &repository, &branch).await;
        let tip = match fetched {
            Ok(tip) => tip,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&path);
                return Err(e);
            }
        };
        let files = match self.tip_files(path.clone(), tip, &folder).await {
            Ok(t) => t,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&path);
                return Err(e);
            }
        };
        if let Ok(Some(n)) = self.folder_format(ws, &files).await? {
            if n > files::FOLDER_FORMAT {
                let _ = std::fs::remove_dir_all(&path);
                return Err(TeamError::permanent("TEAM_FORMAT_NEWER"));
            }
        }
        let binding = Binding {
            workspace_id: ws,
            source_link_id: link,
            host,
            repository,
            branch,
            folder,
            access,
            enabled_at: Timestamp::now(),
            last_checked_at: None,
            last_checked_commit: None,
            access_checked_at: Some(Timestamp::now()),
            available: None,
            to_share: None,
            held_for_check: 0,
            folder_large: false,
            last_error_code: None,
            last_get: None,
            last_share: None,
        };
        if let Err(e) = store::insert_binding(pool, &binding).await {
            let _ = std::fs::remove_dir_all(&path);
            return Err(e);
        }
        self.journal(
            "turned_on",
            ws,
            "This workspace now shares knowledge with a team.",
            serde_json::json!({ "workspace_id": ws, "source_link_id": link, "access": access }),
        )
        .await;
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), access = store::access_text(access), "team.turned_on");
        self.changed(ws).await;
        if let Err(e) = self.check(ws).await {
            tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code, "team.check_failed");
        }
        Ok(binding)
    }

    // ---- §C turning it off ----

    pub async fn turn_off(&self, ws: WorkspaceId) -> Result<(), TeamError> {
        let guard = self.workspace_lock(ws);
        let _held = guard.lock().await;
        // TW-H step 4: on a team workspace, turning off is leaving.
        if self.team_row(ws).await?.is_some() {
            return self.leave_locked(ws).await;
        }
        self.turn_off_locked(ws, "person").await
    }

    pub(crate) async fn turn_off_locked(
        &self,
        ws: WorkspaceId,
        reason: &str,
    ) -> Result<(), TeamError> {
        let pool = self.pool()?;
        if store::binding(pool, ws).await?.is_none() {
            return Err(TeamError::permanent("TEAM_NOT_ON"));
        }
        if let Some(open) = store::open_share(pool, ws).await? {
            match open.state.as_str() {
                "proposed" => {
                    let _ = self.deps.push.withdraw_team_share(open.id).await;
                    store::set_share_state(pool, open.id, "abandoned", None).await?;
                }
                "approved" | "sending" => return Err(TeamError::permanent("TEAM_SHARE_PENDING")),
                _ => {}
            }
        }
        if reason == "link_removed" {
            // The team workspace's own rows go with the folder they describe.
            self.delete_team_rows(ws).await?;
        }
        store::delete_binding(pool, ws).await?;
        let _ = std::fs::remove_dir_all(self.clone_path(ws));
        lock(&self.errors).remove(&ws);
        self.journal(
            "turned_off",
            ws,
            "This workspace no longer shares knowledge with a team.",
            serde_json::json!({ "workspace_id": ws, "reason": reason }),
        )
        .await;
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), reason, "team.turned_off");
        self.changed(ws).await;
        Ok(())
    }

    // ---- §H long commands ----

    /// §H: validates, starts the work in the background and returns the status with `busy` set. A
    /// long command already running returns the status as it is.
    pub(crate) async fn start_long<F, Fut>(
        &self,
        ws: WorkspaceId,
        busy: TeamBusy,
        work: F,
    ) -> Result<TeamKnowledgeStatus, TeamError>
    where
        F: FnOnce(Arc<TeamKnowledgeService>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), TeamError>> + Send + 'static,
    {
        let running = {
            let mut map = lock(&self.busy);
            let running = map.contains_key(&ws);
            if !running {
                map.insert(ws, busy);
            }
            running
        };
        if running {
            return self.status(ws).await;
        }
        let Some(me) = self.me.upgrade() else {
            lock(&self.busy).remove(&ws);
            return Err(TeamError::datastore());
        };
        let status = self.status(ws).await;
        tokio::spawn(async move {
            let guard = me.workspace_lock(ws);
            let held = guard.lock().await;
            let outcome = work(Arc::clone(&me)).await;
            drop(held);
            me.finish_long(ws, outcome).await;
        });
        status
    }

    pub(crate) async fn finish_long(&self, ws: WorkspaceId, outcome: Result<(), TeamError>) {
        let code = outcome.err().map(|e| e.code);
        match code {
            Some(code) => {
                lock(&self.errors).insert(ws, code);
            }
            None => {
                lock(&self.errors).remove(&ws);
            }
        }
        if let Ok(pool) = self.pool() {
            let _ = store::set_last_error(pool, ws, code).await;
        }
        lock(&self.busy).remove(&ws);
        self.changed(ws).await;
    }

    /// §H step 1's refusals for a share.
    pub(crate) async fn bound(&self, ws: WorkspaceId) -> Result<Binding, TeamError> {
        store::binding(self.pool()?, ws)
            .await?
            .ok_or_else(|| TeamError::permanent("TEAM_NOT_ON"))
    }

    pub async fn check_now(&self, ws: WorkspaceId) -> Result<TeamKnowledgeStatus, TeamError> {
        self.bound(ws).await?;
        self.start_long(ws, TeamBusy::Checking, move |me| async move {
            me.check(ws).await
        })
        .await
    }

    pub async fn get(&self, ws: WorkspaceId) -> Result<TeamKnowledgeStatus, TeamError> {
        self.bound(ws).await?;
        self.start_long(ws, TeamBusy::Getting, move |me| async move {
            me.get_now(ws).await.map(|_| ())
        })
        .await
    }

    pub async fn share(&self, ws: WorkspaceId) -> Result<TeamKnowledgeStatus, TeamError> {
        self.share_refusals(ws).await?;
        self.start_long(ws, TeamBusy::Sharing, move |me| async move {
            me.share_now(ws, store::ShareScope::All).await.map(|_| ())
        })
        .await
    }

    /// §E step 1 (and TW-I step 1).
    pub(crate) async fn share_refusals(&self, ws: WorkspaceId) -> Result<Binding, TeamError> {
        let b = self.bound(ws).await?;
        if b.access == TeamAccess::ReadOnly {
            return Err(TeamError::permanent("TEAM_READ_ONLY"));
        }
        if store::open_share(self.pool()?, ws).await?.is_some() {
            return Err(TeamError::permanent("TEAM_SHARE_PENDING"));
        }
        // spec: ordin-desktop/components/TeamKnowledgeService#TW-I
        if let Some(row) = self.team_row(ws).await? {
            if row.state == "proposed" {
                return Err(TeamError::permanent("TEAM_SHARE_PENDING"));
            }
        }
        Ok(b)
    }

    /// §D step 1: access is re-read at most once an hour; a change is stored.
    pub(crate) async fn recheck_access(&self, b: &mut Binding) {
        let due = b.access_checked_at.is_none_or(|at| {
            Timestamp::now().unix_millis() - at.unix_millis() >= ACCESS_RECHECK.as_millis() as i64
        });
        if !due {
            return;
        }
        if let Ok(access) = self.access_of(&b.host, &b.repository).await {
            if let Ok(pool) = self.pool() {
                let _ = store::set_access(pool, b.workspace_id, access).await;
            }
            b.access = access;
        }
    }
}

/// The 2C binding view of the own folder (SA-TEAMR-01: its folder id is the workspace id value).
pub(crate) fn binding_view(b: &Binding) -> TeamKnowledgeBinding {
    TeamKnowledgeBinding {
        id: TeamFolderId(b.workspace_id.0),
        workspace_id: b.workspace_id,
        role: TeamFolderRole::Own,
        source_link_id: Some(b.source_link_id),
        host: b.host.clone(),
        repository: b.repository.clone(),
        branch: b.branch.clone(),
        folder: b.folder.clone(),
        access: b.access,
        added_by_person: true,
        company_key: None,
        display_name: None,
        review_required: false,
        person_may_stop: true,
        enabled_at: b.enabled_at,
    }
}
