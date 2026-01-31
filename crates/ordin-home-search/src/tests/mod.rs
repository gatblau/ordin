//! Unit tests: the module over doubles of its dependencies and a real SQLite datastore in a
//! temporary folder. The integration test (`tests/workspaces_integration.rs`) uses the real
//! File Service, Workspace Service and adapters.

mod acceptance;
mod error_table;
mod logic;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_files::{
    FileError, IndexCoverage, PlaceChange, PlaceChangeKind, SearchHit, SearchResults,
};
use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS};
use ordin_knowledge::{KnowledgeError, KnowledgeReader, PageHit, PageSummary};
use ordin_types::common::InputUsed;
use ordin_types::domain::places::{
    KnowledgeAuthor, KnowledgePageState, Place, PlaceKind, SourceLink, SourceLinkKind,
    SourceLinkState, Workspace, WorkspaceState,
};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{KnowledgePageId, PlaceId, SourceLinkId, WorkspaceId};
use ordin_workspaces::{OriginChanged, OriginVersion, WorkspaceError, WorkspaceListFilter};
use sqlx::SqlitePool;
use tokio::sync::broadcast;

use crate::ports::{FilesPort, Ports, RemoteChange, RemotePort, VersionRead, WorkspacesPort};
use crate::{
    GoToKind, GoToResult, HomeSearch, HomeSourceError, JournalReader, RunAgainSubject, RunSummary,
    SearchSource, WaitingItem,
};

pub(crate) fn at(raw: &str) -> Timestamp {
    Timestamp(parse_rfc3339_ms_utc(raw).expect("timestamp"))
}

/// A folder removed when dropped.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-home-search-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------- doubles

#[derive(Default)]
pub(crate) struct FakeReader {
    pub waiting: Mutex<Option<Vec<WaitingItem>>>,
    pub running: Mutex<Option<Vec<RunSummary>>>,
    pub finished: Mutex<Option<Vec<RunSummary>>>,
    pub inputs: Mutex<Option<Result<Vec<InputUsed>, HomeSourceError>>>,
}

fn take<T: Clone>(m: &Mutex<Option<T>>) -> Result<T, HomeSourceError> {
    m.lock()
        .expect("lock")
        .clone()
        .ok_or(HomeSourceError::Unavailable)
}

#[async_trait::async_trait]
impl JournalReader for FakeReader {
    async fn waiting_items(&self) -> Result<Vec<WaitingItem>, HomeSourceError> {
        take(&self.waiting)
    }
    async fn running(&self) -> Result<Vec<RunSummary>, HomeSourceError> {
        take(&self.running)
    }
    async fn finished_since(
        &self,
        _since: Timestamp,
        _limit: u32,
    ) -> Result<Vec<RunSummary>, HomeSourceError> {
        take(&self.finished)
    }
    async fn run_inputs(&self, _run: RunAgainSubject) -> Result<Vec<InputUsed>, HomeSourceError> {
        self.inputs
            .lock()
            .expect("lock")
            .clone()
            .unwrap_or(Err(HomeSourceError::NotFound))
    }
}

#[derive(Default)]
pub(crate) struct FakeKnowledge {
    pub hits: Mutex<Vec<PageHit>>,
    pub out_of_date: Mutex<Option<Vec<PageSummary>>>,
    pub versions: Mutex<HashMap<KnowledgePageId, Option<String>>>,
    pub fail: Mutex<bool>,
}

#[async_trait::async_trait]
impl KnowledgeReader for FakeKnowledge {
    async fn search_pages(
        &self,
        _workspaces: &[WorkspaceId],
        _query: &str,
        _limit: u32,
    ) -> Result<Vec<PageHit>, KnowledgeError> {
        if *self.fail.lock().expect("lock") {
            return Err(KnowledgeError::DbUnavailable);
        }
        Ok(self.hits.lock().expect("lock").clone())
    }
    async fn out_of_date_since(
        &self,
        _since: Timestamp,
    ) -> Result<Vec<PageSummary>, KnowledgeError> {
        self.out_of_date
            .lock()
            .expect("lock")
            .clone()
            .ok_or(KnowledgeError::DbUnavailable)
    }
    async fn page_version(&self, id: KnowledgePageId) -> Result<Option<String>, KnowledgeError> {
        if *self.fail.lock().expect("lock") {
            return Err(KnowledgeError::DbUnavailable);
        }
        Ok(self
            .versions
            .lock()
            .expect("lock")
            .get(&id)
            .cloned()
            .flatten())
    }
}

pub(crate) struct FakeFiles {
    pub sender: broadcast::Sender<PlaceChange>,
    pub hits: Mutex<Vec<SearchHit>>,
    pub coverage: Mutex<IndexCoverage>,
    pub hashes: Mutex<HashMap<(PlaceId, String), String>>,
    pub fail: Mutex<bool>,
    pub delay: Mutex<Duration>,
}

impl Default for FakeFiles {
    fn default() -> Self {
        FakeFiles {
            sender: broadcast::channel(16).0,
            hits: Mutex::new(Vec::new()),
            coverage: Mutex::new(IndexCoverage {
                indexed: 10,
                total: 10,
            }),
            hashes: Mutex::new(HashMap::new()),
            fail: Mutex::new(false),
            delay: Mutex::new(Duration::ZERO),
        }
    }
}

#[async_trait::async_trait]
impl FilesPort for FakeFiles {
    fn subscribe_changes(&self) -> broadcast::Receiver<PlaceChange> {
        self.sender.subscribe()
    }
    async fn search(
        &self,
        place_ids: &[PlaceId],
        _query: &str,
        _limit: u32,
    ) -> Result<SearchResults, FileError> {
        let delay = *self.delay.lock().expect("lock");
        tokio::time::sleep(delay).await;
        if *self.fail.lock().expect("lock") {
            return Err(FileError::DatastoreUnavailable);
        }
        Ok(SearchResults {
            hits: self
                .hits
                .lock()
                .expect("lock")
                .iter()
                .filter(|h| place_ids.contains(&h.place_id))
                .cloned()
                .collect(),
            coverage: *self.coverage.lock().expect("lock"),
        })
    }
    async fn file_hash(&self, place_id: PlaceId, path: &str) -> Result<String, FileError> {
        if *self.fail.lock().expect("lock") {
            return Err(FileError::DatastoreUnavailable);
        }
        self.hashes
            .lock()
            .expect("lock")
            .get(&(place_id, path.to_owned()))
            .cloned()
            .ok_or(FileError::NotFound)
    }
}

#[derive(Default)]
pub(crate) struct FakeWorkspaces {
    pub list: Mutex<Vec<Workspace>>,
    pub places: Mutex<HashMap<WorkspaceId, Vec<Place>>>,
    pub links: Mutex<HashMap<WorkspaceId, Vec<SourceLink>>>,
    pub origin: Mutex<Vec<OriginChanged>>,
    pub fail_list: Mutex<bool>,
    pub fail_origin: Mutex<bool>,
}

#[async_trait::async_trait]
impl WorkspacesPort for FakeWorkspaces {
    async fn list(&self, filter: WorkspaceListFilter) -> Result<Vec<Workspace>, WorkspaceError> {
        if *self.fail_list.lock().expect("lock") {
            return Err(WorkspaceError::DatastoreUnavailable);
        }
        Ok(self
            .list
            .lock()
            .expect("lock")
            .iter()
            .filter(|w| match filter {
                WorkspaceListFilter::Active => w.state == WorkspaceState::Active,
                WorkspaceListFilter::Archived => w.state == WorkspaceState::Archived,
                WorkspaceListFilter::InBin => w.state == WorkspaceState::InBin,
                WorkspaceListFilter::NotInBin => w.state != WorkspaceState::InBin,
            })
            .cloned()
            .collect())
    }
    async fn places(&self, id: WorkspaceId) -> Result<Vec<Place>, WorkspaceError> {
        Ok(self
            .places
            .lock()
            .expect("lock")
            .get(&id)
            .cloned()
            .unwrap_or_default())
    }
    async fn source_links(&self, id: WorkspaceId) -> Result<Vec<SourceLink>, WorkspaceError> {
        Ok(self
            .links
            .lock()
            .expect("lock")
            .get(&id)
            .cloned()
            .unwrap_or_default())
    }
    async fn origin_changes_since(
        &self,
        _since: Timestamp,
    ) -> Result<Vec<OriginChanged>, WorkspaceError> {
        if *self.fail_origin.lock().expect("lock") {
            return Err(WorkspaceError::DatastoreUnavailable);
        }
        Ok(self.origin.lock().expect("lock").clone())
    }
}

#[derive(Default)]
pub(crate) struct FakeRemote {
    pub changes: Mutex<Vec<RemoteChange>>,
    pub fail: Mutex<bool>,
    pub versions: Mutex<HashMap<String, VersionRead>>,
    pub asked: Mutex<Vec<Vec<SourceLinkId>>>,
}

#[async_trait::async_trait]
impl RemotePort for FakeRemote {
    async fn version(&self, location: &str) -> VersionRead {
        self.versions
            .lock()
            .expect("lock")
            .get(location)
            .cloned()
            .unwrap_or(VersionRead::Failed)
    }
    async fn changes_since(
        &self,
        links: &[SourceLinkId],
        _since: Timestamp,
    ) -> Result<Vec<RemoteChange>, String> {
        self.asked.lock().expect("lock").push(links.to_vec());
        if *self.fail.lock().expect("lock") {
            return Err("SP_UNAVAILABLE".to_owned());
        }
        Ok(self.changes.lock().expect("lock").clone())
    }
}

/// A search source that answers after `delay`.
pub(crate) struct SlowSource {
    pub delay: Duration,
    pub results: Vec<GoToResult>,
}

#[async_trait::async_trait]
impl SearchSource for SlowSource {
    fn group(&self) -> GoToKind {
        GoToKind::Agent
    }
    async fn find(&self, _query: &str, _limit: u32) -> Vec<GoToResult> {
        tokio::time::sleep(self.delay).await;
        self.results.clone()
    }
}

// ---------------------------------------------------------------- fixture

pub(crate) struct Env {
    pub _dir: TempDir,
    pub pool: SqlitePool,
    pub store: Datastore,
    pub reader: Arc<FakeReader>,
    pub knowledge: Arc<FakeKnowledge>,
    pub files: Arc<FakeFiles>,
    pub workspaces: Arc<FakeWorkspaces>,
    pub sharepoint: Arc<FakeRemote>,
    pub confluence: Arc<FakeRemote>,
    pub home: Arc<HomeSearch>,
}

impl Env {
    pub async fn new() -> Env {
        Env::with_confluence(true).await
    }

    pub async fn with_confluence(confluence: bool) -> Env {
        let dir = TempDir::new();
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: dir.0.join("journal.db"),
            backup_dir: dir.0.join("backups"),
        })
        .await
        .expect("datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).expect("journal");
        registry
            .register(HomeSearch::migrations())
            .expect("home_search");
        registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .expect("migrations");
        let pool = store.sqlite().expect("sqlite").clone();
        let reader = Arc::new(FakeReader::default());
        let knowledge = Arc::new(FakeKnowledge::default());
        *knowledge.out_of_date.lock().expect("lock") = Some(Vec::new());
        let files = Arc::new(FakeFiles::default());
        let workspaces = Arc::new(FakeWorkspaces::default());
        let sharepoint = Arc::new(FakeRemote::default());
        let confluence_double = Arc::new(FakeRemote::default());
        *reader.waiting.lock().expect("lock") = Some(Vec::new());
        *reader.running.lock().expect("lock") = Some(Vec::new());
        *reader.finished.lock().expect("lock") = Some(Vec::new());
        let ports = Ports {
            reader: reader.clone(),
            knowledge: knowledge.clone(),
            files: files.clone(),
            workspaces: workspaces.clone(),
            sharepoint: sharepoint.clone(),
            confluence: confluence.then(|| confluence_double.clone() as Arc<dyn RemotePort>),
        };
        let home = HomeSearch::with_ports(pool.clone(), ports);
        Env {
            _dir: dir,
            pool,
            store,
            reader,
            knowledge,
            files,
            workspaces,
            sharepoint,
            confluence: confluence_double,
            home,
        }
    }

    /// A workspace with its own folder place, listed by the Workspace Service double.
    pub fn workspace(&self, name: &str, state: WorkspaceState) -> (WorkspaceId, PlaceId) {
        let id = WorkspaceId::new();
        let place = PlaceId::new();
        self.workspaces.list.lock().expect("lock").push(Workspace {
            id,
            name: name.to_owned(),
            own_folder_place_id: place,
            state,
            last_seen_at: None,
            moved_to_bin_at: None,
            created_at: Timestamp::now(),
        });
        self.add_place(id, place, PlaceKind::WorkspaceFolder);
        (id, place)
    }

    pub fn add_place(&self, ws: WorkspaceId, place: PlaceId, kind: PlaceKind) {
        self.workspaces
            .places
            .lock()
            .expect("lock")
            .entry(ws)
            .or_default()
            .push(Place {
                id: place,
                friendly_name: "Folder".to_owned(),
                path: "/tmp/folder".to_owned(),
                kind,
                added_at: Timestamp::now(),
            });
    }

    /// A source link of `kind` whose working copy is `place` (when given).
    pub fn link(
        &self,
        ws: WorkspaceId,
        kind: SourceLinkKind,
        name: &str,
        place: Option<PlaceId>,
    ) -> SourceLinkId {
        let id = SourceLinkId::new();
        if let Some(p) = place {
            self.add_place(ws, p, PlaceKind::WorkingCopy);
        }
        self.workspaces
            .links
            .lock()
            .expect("lock")
            .entry(ws)
            .or_default()
            .push(SourceLink {
                id,
                workspace_id: ws,
                kind,
                origin_location: "https://acme.example/contracts".to_owned(),
                display_name: name.to_owned(),
                place_id: place,
                state: SourceLinkState::Active,
                added_at: Timestamp::now(),
                last_read_at: None,
            });
        id
    }

    pub fn origin_change(
        &self,
        ws: WorkspaceId,
        link: SourceLinkId,
        kind: SourceLinkKind,
        path: &str,
        changed_at: Timestamp,
        removed: bool,
    ) {
        self.workspaces
            .origin
            .lock()
            .expect("lock")
            .push(OriginChanged {
                workspace_id: ws,
                source_link_id: link,
                link_kind: kind,
                origin_path: path.to_owned(),
                version: OriginVersion {
                    version: "v2".to_owned(),
                    changed_by: Some("Sam Lee".to_owned()),
                    changed_at: Some(changed_at),
                },
                removed,
            });
    }

    pub fn page(&self, ws: WorkspaceId, title: &str, changed: Timestamp) -> PageSummary {
        PageSummary {
            workspace_id: ws,
            id: KnowledgePageId::new(),
            title: title.to_owned(),
            state: KnowledgePageState::MayBeOutOfDate,
            written_by: KnowledgeAuthor::Person,
            last_changed_at: changed,
            shared_by: None,
            shared_at: None,
            cites_other_computer: false,
            tags: Vec::new(),
            folder: None,
            folder_description: None,
            set_by_company: None,
            folder_id: None,
            read_only: false,
            adopted_from: None,
            adoption_mark: None,
            page_type: None,
            description: None,
            trust: ordin_types::wire::knowledge_query::PageTier::Unverified,
            status: None,
            past_stale_after: false,
        }
    }

    pub fn hit(&self, ws: WorkspaceId, title: &str, snippet: &str) -> PageHit {
        PageHit {
            workspace_id: ws,
            id: KnowledgePageId::new(),
            title: title.to_owned(),
            state: KnowledgePageState::Current,
            snippet: snippet.to_owned(),
            rank: 1.0,
        }
    }

    pub async fn set_home_opened(&self, when: Timestamp) {
        crate::store::set_home_opened(&self.pool, when)
            .await
            .expect("home opened");
    }
}

pub(crate) fn place_change(
    place: PlaceId,
    path: &str,
    kind: PlaceChangeKind,
    by_ordin: bool,
) -> PlaceChange {
    PlaceChange {
        place_id: place,
        path: path.to_owned(),
        change: kind,
        observed_at: Timestamp::now(),
        by_ordin,
        version: None,
        origin: None,
    }
}

pub(crate) fn file_hit(
    place: PlaceId,
    path: &str,
    name_match: bool,
    snippet: Option<&str>,
) -> SearchHit {
    SearchHit {
        place_id: place,
        path: path.to_owned(),
        name_match,
        snippet: snippet.map(str::to_owned),
        changed_at: Timestamp::now(),
    }
}
