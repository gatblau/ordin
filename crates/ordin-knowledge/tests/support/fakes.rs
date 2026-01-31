//! In-memory stand-ins of the File, Workspace, Rules, Memory, Push, Folders and Identity ports and
//! of the agent profile's ports, written for this crate's tests (Gherkin *Background*: "in-memory
//! fakes"). Shared by the in-crate tests (through `#[path]`) and the integration tests.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_files::{
    BinTicket, Cursor, FileBytes, FileEntry, FileError, FileVersion, FileVersionOrigin, FolderSort,
    Page, PlaceChange, Sha256Hex, WriteVersion,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_knowledge::{
    AgentKnowledgeDeps, AgentKnowledgeFile, ChatHistory, ChatHistoryError, ChatHistoryPage,
    KnowledgeDeps, KnowledgeFiles, KnowledgeFolders, KnowledgeIdentity, KnowledgeMemory,
    KnowledgeMetrics, KnowledgePush, KnowledgeRules, KnowledgeRunContext, KnowledgeScopeSettings,
    KnowledgeService, KnowledgeSourceConfig, KnowledgeWorkspaces, PackageError, ProgramKind,
    RulesUnavailable, ScopeChat, SettingsReadError, SourceReadCheck, WorkspaceAgent,
};
use ordin_memory_service::{
    AdoptionLabel, ContentCheck, FolderLabel, MemoryError, PageGraph, PageSection,
    PersonalDataFinding,
};
use ordin_push::{PublishCopyRequest, PushError};
use ordin_types::domain::places::{SourceLinkKind, Workspace, WorkspaceState};
use ordin_types::time::Timestamp;
use ordin_types::{
    AgentInstanceId, FileVersionId, KnowledgePageId, PlaceId, PublishedCopyId, RunId, ScopeId,
    TeamFolderId, WorkspaceId,
};
use ordin_workspaces::{OriginChanged, OriginVersion, WorkspaceError};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn hash(bytes: &[u8]) -> Sha256Hex {
    Sha256Hex(
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

/// A folder removed when dropped.
pub struct TempDir(pub std::path::PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("ordin-knowledge-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, Clone)]
pub struct StoredVersion {
    pub version: FileVersion,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
struct FilesState {
    /// (place, path) → versions, oldest first; `present` says whether the file exists now.
    files: BTreeMap<(PlaceId, String), (Vec<StoredVersion>, bool)>,
    places: HashSet<PlaceId>,
    bin: HashMap<FileVersionId, Vec<u8>>,
    fail_writes: bool,
    fail_permanent: bool,
    conflicts: HashMap<String, u32>,
    agent_files: HashMap<AgentInstanceId, BTreeMap<String, Vec<u8>>>,
    moves: Vec<(String, String)>,
}

pub struct FakeFiles {
    state: Mutex<FilesState>,
    pub changes: broadcast::Sender<PlaceChange>,
}

impl FakeFiles {
    pub fn new() -> FakeFiles {
        let (changes, _) = broadcast::channel(256);
        FakeFiles {
            state: Mutex::new(FilesState::default()),
            changes,
        }
    }

    pub fn add_place(&self, place: PlaceId) {
        lock(&self.state).places.insert(place);
    }

    pub fn fail_writes(&self, on: bool) {
        lock(&self.state).fail_writes = on;
    }

    pub fn fail_permanently(&self, on: bool) {
        lock(&self.state).fail_permanent = on;
    }

    /// The next `n` writes with an `expected_hash` to `path` meet a concurrent edit.
    pub fn conflict_next(&self, path: &str, n: u32) {
        lock(&self.state).conflicts.insert(path.to_owned(), n);
    }

    pub fn add_agent_file(&self, agent: AgentInstanceId, path: &str, text: &str) {
        lock(&self.state)
            .agent_files
            .entry(agent)
            .or_default()
            .insert(path.to_owned(), text.as_bytes().to_vec());
    }

    fn push_version(
        state: &mut FilesState,
        place: PlaceId,
        path: &str,
        bytes: Vec<u8>,
        origin: FileVersionOrigin,
    ) -> FileVersion {
        let entry = state
            .files
            .entry((place, path.to_owned()))
            .or_insert_with(|| (Vec::new(), true));
        let version = FileVersion {
            id: FileVersionId::new(),
            place_id: place,
            path: path.to_owned(),
            version: u32::try_from(entry.0.len() + 1).unwrap_or(u32::MAX),
            hash: hash(&bytes),
            origin,
            created_at: Timestamp::now(),
        };
        entry.0.push(StoredVersion {
            version: version.clone(),
            bytes,
        });
        entry.1 = true;
        version
    }

    /// Another app writes the file: a new version with origin Person.
    pub fn external_write(&self, place: PlaceId, path: &str, text: &str) -> FileVersion {
        let mut s = lock(&self.state);
        Self::push_version(
            &mut s,
            place,
            path,
            text.as_bytes().to_vec(),
            FileVersionOrigin::Person,
        )
    }

    pub fn external_remove(&self, place: PlaceId, path: &str) {
        if let Some(e) = lock(&self.state).files.get_mut(&(place, path.to_owned())) {
            e.1 = false;
        }
    }

    pub fn external_rename(&self, place: PlaceId, from: &str, to: &str) {
        let mut s = lock(&self.state);
        if let Some(e) = s.files.remove(&(place, from.to_owned())) {
            s.files.insert((place, to.to_owned()), e);
        }
    }

    /// `files.restore_version`: the given version's bytes become the next version.
    pub fn restore_version(&self, place: PlaceId, path: &str, version: u32) -> FileVersion {
        let mut s = lock(&self.state);
        let bytes = s.files[&(place, path.to_owned())].0[(version - 1) as usize]
            .bytes
            .clone();
        Self::push_version(&mut s, place, path, bytes, FileVersionOrigin::Person)
    }

    pub fn text(&self, place: PlaceId, path: &str) -> Option<String> {
        let s = lock(&self.state);
        let (versions, present) = s.files.get(&(place, path.to_owned()))?;
        if !present {
            return None;
        }
        versions
            .last()
            .map(|v| String::from_utf8_lossy(&v.bytes).into_owned())
    }

    pub fn all_versions(&self, place: PlaceId, path: &str) -> Vec<StoredVersion> {
        lock(&self.state)
            .files
            .get(&(place, path.to_owned()))
            .map(|e| e.0.clone())
            .unwrap_or_default()
    }

    pub fn paths(&self, place: PlaceId) -> Vec<String> {
        lock(&self.state)
            .files
            .iter()
            .filter(|((p, _), (_, present))| *p == place && *present)
            .map(|((_, path), _)| path.clone())
            .collect()
    }

    pub fn moves(&self) -> Vec<(String, String)> {
        lock(&self.state).moves.clone()
    }

    pub fn write_count(&self, place: PlaceId, path: &str) -> usize {
        self.all_versions(place, path).len()
    }

    fn check_place(&self, place: PlaceId) -> Result<(), FileError> {
        if lock(&self.state).places.contains(&place) {
            Ok(())
        } else {
            Err(FileError::PlaceNotFound)
        }
    }
}

#[async_trait::async_trait]
impl KnowledgeFiles for FakeFiles {
    async fn write_version(&self, w: WriteVersion) -> Result<FileVersion, FileError> {
        self.check_place(w.place_id)?;
        let mut s = lock(&self.state);
        if s.fail_writes {
            return Err(FileError::WriteFailed);
        }
        if s.fail_permanent {
            return Err(FileError::ProtectedLocation);
        }
        if let Some(expected) = &w.expected_hash {
            if let Some(n) = s.conflicts.get_mut(&w.path) {
                if *n > 0 {
                    *n -= 1;
                    return Err(FileError::ChangedMeanwhile {
                        current: expected.clone(),
                    });
                }
            }
            let current = s
                .files
                .get(&(w.place_id, w.path.clone()))
                .filter(|e| e.1)
                .and_then(|e| e.0.last())
                .map(|v| v.version.hash.clone());
            if current.as_ref() != Some(expected) {
                return Err(FileError::ChangedMeanwhile {
                    current: current.unwrap_or_else(|| expected.clone()),
                });
            }
        }
        Ok(Self::push_version(
            &mut s, w.place_id, &w.path, w.bytes, w.origin,
        ))
    }

    async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError> {
        self.check_place(place_id)?;
        let s = lock(&self.state);
        match s.files.get(&(place_id, path.to_owned())) {
            Some((v, true)) => {
                let last = v.last().ok_or(FileError::NotFound)?;
                Ok(FileBytes {
                    place_id,
                    path: path.to_owned(),
                    bytes: last.bytes.clone(),
                    hash: last.version.hash.clone(),
                    media_type: "text/markdown".to_owned(),
                })
            }
            _ => Err(FileError::NotFound),
        }
    }

    async fn current_version(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<Option<FileVersion>, FileError> {
        self.check_place(place_id)?;
        let s = lock(&self.state);
        Ok(s.files
            .get(&(place_id, path.to_owned()))
            .filter(|e| e.1)
            .and_then(|e| e.0.last())
            .map(|v| v.version.clone()))
    }

    async fn move_to_bin(&self, place_id: PlaceId, path: &str) -> Result<BinTicket, FileError> {
        self.check_place(place_id)?;
        let mut s = lock(&self.state);
        if s.fail_writes {
            return Err(FileError::BinUnavailable);
        }
        let entry = s
            .files
            .get_mut(&(place_id, path.to_owned()))
            .ok_or(FileError::NotFound)?;
        entry.1 = false;
        let last = entry.0.last().cloned().ok_or(FileError::NotFound)?;
        s.bin.insert(last.version.id, last.bytes);
        Ok(BinTicket {
            place_id,
            path: path.to_owned(),
            file_version_id: last.version.id,
        })
    }

    async fn restore_from_bin(&self, ticket: &BinTicket) -> Result<FileVersion, FileError> {
        let mut s = lock(&self.state);
        let bytes = match s.bin.remove(&ticket.file_version_id) {
            Some(b) => b,
            None => s
                .files
                .get(&(ticket.place_id, ticket.path.clone()))
                .and_then(|e| e.0.iter().find(|v| v.version.id == ticket.file_version_id))
                .map(|v| v.bytes.clone())
                .ok_or(FileError::NotFound)?,
        };
        Ok(Self::push_version(
            &mut s,
            ticket.place_id,
            &ticket.path,
            bytes,
            FileVersionOrigin::Person,
        ))
    }

    fn subscribe_changes(&self) -> broadcast::Receiver<PlaceChange> {
        self.changes.subscribe()
    }

    async fn list_agent_knowledge(
        &self,
        agent: AgentInstanceId,
    ) -> Result<Vec<AgentKnowledgeFile>, FileError> {
        Ok(lock(&self.state)
            .agent_files
            .get(&agent)
            .map(|m| {
                m.iter()
                    .map(|(p, b)| AgentKnowledgeFile {
                        relative_path: p.clone(),
                        bytes: b.len() as u64,
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn read_agent_knowledge(
        &self,
        agent: AgentInstanceId,
        relative_path: &str,
    ) -> Result<Vec<u8>, FileError> {
        lock(&self.state)
            .agent_files
            .get(&agent)
            .and_then(|m| m.get(relative_path).cloned())
            .ok_or(FileError::NotFound)
    }

    async fn move_file(
        &self,
        place_id: PlaceId,
        from: &str,
        to: &str,
    ) -> Result<FileVersion, FileError> {
        self.check_place(place_id)?;
        let mut s = lock(&self.state);
        if s.files.get(&(place_id, to.to_owned())).is_some_and(|e| e.1) {
            return Err(FileError::MoveTargetExists);
        }
        let mut entry = s
            .files
            .remove(&(place_id, from.to_owned()))
            .ok_or(FileError::NotFound)?;
        for v in &mut entry.0 {
            v.version.path = to.to_owned();
        }
        let last = entry
            .0
            .last()
            .map(|v| v.version.clone())
            .ok_or(FileError::NotFound)?;
        s.files.insert((place_id, to.to_owned()), entry);
        s.moves.push((from.to_owned(), to.to_owned()));
        Ok(last)
    }

    async fn versions(&self, place_id: PlaceId, path: &str) -> Result<Vec<FileVersion>, FileError> {
        Ok(self
            .all_versions(place_id, path)
            .into_iter()
            .map(|v| v.version)
            .collect())
    }

    async fn list_folder(
        &self,
        place_id: PlaceId,
        folder: &str,
        _sort: FolderSort,
        _cursor: Option<Cursor>,
    ) -> Result<Page<FileEntry>, FileError> {
        self.check_place(place_id)?;
        let prefix = format!("{folder}/");
        let mut seen = HashSet::new();
        let mut items = Vec::new();
        for p in self.paths(place_id) {
            let Some(rest) = p.strip_prefix(&prefix) else {
                continue;
            };
            let (name, is_folder) = match rest.split_once('/') {
                Some((dir, _)) => (dir.to_owned(), true),
                None => (rest.to_owned(), false),
            };
            if !seen.insert(name.clone()) {
                continue;
            }
            items.push(FileEntry {
                place_id,
                path: if is_folder {
                    format!("{prefix}{name}/")
                } else {
                    format!("{prefix}{name}")
                },
                name: name.clone(),
                is_folder,
                kind: String::new(),
                size_bytes: 0,
                changed_at: Timestamp::now(),
                changed_by: None,
                has_versions: true,
            });
        }
        Ok(Page {
            items,
            next_cursor: None,
        })
    }
}

#[derive(Default)]
pub struct FakeWorkspaces {
    pub workspaces: Mutex<HashMap<WorkspaceId, Workspace>>,
    pub origins: Mutex<HashMap<(WorkspaceId, String), OriginVersion>>,
    pub slow_origins: Mutex<bool>,
    pub agents: Mutex<HashMap<WorkspaceId, Vec<WorkspaceAgent>>>,
    pub origin_tx: Mutex<Option<broadcast::Sender<OriginChanged>>>,
}

impl FakeWorkspaces {
    pub fn add(&self, name: &str, place: PlaceId) -> WorkspaceId {
        let id = WorkspaceId::new();
        lock(&self.workspaces).insert(
            id,
            Workspace {
                id,
                name: name.to_owned(),
                own_folder_place_id: place,
                state: WorkspaceState::Active,
                last_seen_at: None,
                moved_to_bin_at: None,
                created_at: Timestamp::now(),
            },
        );
        id
    }

    pub fn set_state(&self, id: WorkspaceId, state: WorkspaceState) {
        if let Some(w) = lock(&self.workspaces).get_mut(&id) {
            w.state = state;
        }
    }

    pub fn set_origin(&self, w: WorkspaceId, location: &str, version: &str, by: Option<&str>) {
        lock(&self.origins).insert(
            (w, location.to_owned()),
            OriginVersion {
                version: version.to_owned(),
                changed_by: by.map(str::to_owned),
                changed_at: Some(Timestamp::now()),
            },
        );
    }
}

#[async_trait::async_trait]
impl KnowledgeWorkspaces for FakeWorkspaces {
    async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        lock(&self.workspaces)
            .get(&id)
            .cloned()
            .ok_or(WorkspaceError::NotFound)
    }

    async fn list_not_in_bin(&self) -> Result<Vec<Workspace>, WorkspaceError> {
        Ok(lock(&self.workspaces)
            .values()
            .filter(|w| w.state != WorkspaceState::InBin)
            .cloned()
            .collect())
    }

    async fn origin_version(
        &self,
        workspace_id: WorkspaceId,
        _link_kind: SourceLinkKind,
        origin_path: &str,
    ) -> Result<Option<OriginVersion>, WorkspaceError> {
        if *lock(&self.slow_origins) {
            tokio::time::sleep(Duration::from_secs(6)).await;
        }
        Ok(lock(&self.origins)
            .get(&(workspace_id, origin_path.to_owned()))
            .cloned())
    }

    fn subscribe_origin_changes(&self) -> broadcast::Receiver<OriginChanged> {
        let mut tx = lock(&self.origin_tx);
        tx.get_or_insert_with(|| broadcast::channel(64).0)
            .subscribe()
    }

    async fn agents_in_workspace(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<WorkspaceAgent>, PackageError> {
        Ok(lock(&self.agents).get(&id).cloned().unwrap_or_default())
    }
}

#[derive(Default)]
pub struct FakeRules {
    /// (agent, location) pairs the Rules Engine refuses; `*` refuses every location of the agent.
    pub refused: Mutex<HashSet<(AgentInstanceId, String)>>,
    pub unavailable: Mutex<bool>,
    /// Places that exist on this computer; a `place` source of any other place is refused.
    pub known_places: Mutex<HashSet<PlaceId>>,
}

#[async_trait::async_trait]
impl KnowledgeRules for FakeRules {
    async fn may_read_source(&self, check: &SourceReadCheck) -> Result<bool, RulesUnavailable> {
        if *lock(&self.unavailable) {
            return Err(RulesUnavailable);
        }
        let refused = lock(&self.refused);
        if refused.contains(&(check.agent_instance_id, check.location.clone()))
            || refused.contains(&(check.agent_instance_id, "*".to_owned()))
        {
            return Ok(false);
        }
        if check.origin == "place" {
            let known = lock(&self.known_places);
            let place = check
                .location
                .strip_prefix("place:")
                .and_then(|r| r.split(':').next())
                .and_then(|p| PlaceId::parse(p).ok());
            return Ok(place.is_some_and(|p| known.contains(&p)));
        }
        Ok(true)
    }
}

#[derive(Debug, Clone)]
pub struct IndexCall {
    pub workspace_id: WorkspaceId,
    pub folder_id: Option<TeamFolderId>,
    pub page_id: KnowledgePageId,
    pub sections: Vec<(String, String)>,
    pub version: String,
    pub graph: PageGraph,
}

#[derive(Default)]
pub struct FakeMemory {
    pub calls: Mutex<Vec<IndexCall>>,
    pub removed: Mutex<Vec<KnowledgePageId>>,
    /// Each `remove_page` call's workspace, read folder and page (KNOW-REC-1).
    pub removed_with: Mutex<Vec<(WorkspaceId, Option<TeamFolderId>, KnowledgePageId)>>,
    /// Each `check_content` call's workspace (KNOW-REC-1).
    pub checked_in: Mutex<Vec<WorkspaceId>>,
    pub source_changes: Mutex<Vec<(String, String)>>,
    pub fail_index: Mutex<u32>,
    pub fail_remove: Mutex<u32>,
    /// A text holding the key gets the finding.
    pub personal: Mutex<Vec<String>>,
    pub instruction: Mutex<Vec<String>>,
    pub check_unavailable: Mutex<bool>,
}

impl FakeMemory {
    pub fn calls_for(&self, page: KnowledgePageId) -> Vec<IndexCall> {
        lock(&self.calls)
            .iter()
            .filter(|c| c.page_id == page)
            .cloned()
            .collect()
    }
}

fn mem_err() -> MemoryError {
    MemoryError::Unavailable {
        code: "MEM_STORE_UNAVAILABLE",
        retryable: true,
    }
}

#[async_trait::async_trait]
impl KnowledgeMemory for FakeMemory {
    async fn index_page_sections(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
        sections: Vec<PageSection>,
        version: String,
        graph: PageGraph,
    ) -> Result<(), MemoryError> {
        {
            let mut f = lock(&self.fail_index);
            if *f > 0 {
                *f -= 1;
                return Err(mem_err());
            }
        }
        lock(&self.calls).push(IndexCall {
            workspace_id,
            folder_id: None,
            page_id,
            sections: sections.into_iter().map(|s| (s.heading, s.text)).collect(),
            version,
            graph,
        });
        Ok(())
    }

    async fn remove_page(
        &self,
        workspace_id: WorkspaceId,
        folder_id: Option<TeamFolderId>,
        page_id: KnowledgePageId,
    ) -> Result<(), MemoryError> {
        lock(&self.removed_with).push((workspace_id, folder_id, page_id));
        {
            let mut f = lock(&self.fail_remove);
            if *f > 0 {
                *f -= 1;
                return Err(mem_err());
            }
        }
        lock(&self.removed).push(page_id);
        Ok(())
    }

    async fn source_changed(
        &self,
        origin_ref: &str,
        current_version: &str,
    ) -> Result<u32, MemoryError> {
        lock(&self.source_changes).push((origin_ref.to_owned(), current_version.to_owned()));
        Ok(0)
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
        lock(&self.calls).push(IndexCall {
            workspace_id,
            folder_id: Some(folder_id),
            page_id,
            sections: sections.into_iter().map(|s| (s.heading, s.text)).collect(),
            version,
            graph,
        });
        Ok(())
    }

    async fn check_content(
        &self,
        workspace_id: WorkspaceId,
        text: &str,
    ) -> Result<ContentCheck, MemoryError> {
        lock(&self.checked_in).push(workspace_id);
        if *lock(&self.check_unavailable) {
            return Err(mem_err());
        }
        let personal = lock(&self.personal)
            .iter()
            .any(|k| text.contains(k.as_str()));
        let instruction = lock(&self.instruction)
            .iter()
            .any(|k| text.contains(k.as_str()));
        Ok(ContentCheck {
            personal_data: if personal {
                PersonalDataFinding::Review
            } else {
                PersonalDataFinding::None
            },
            instruction_like: instruction,
        })
    }
}

#[derive(Default)]
pub struct FakePush {
    pub requests: Mutex<Vec<PublishCopyRequest>>,
    pub fail: Mutex<bool>,
}

#[async_trait::async_trait]
impl KnowledgePush for FakePush {
    async fn publish_copy(&self, req: PublishCopyRequest) -> Result<PublishedCopyId, PushError> {
        if *lock(&self.fail) {
            return Err(PushError {
                code: "PUSH_REQUEST_INVALID",
                retryable: false,
                action: None,
                detail: String::new(),
            });
        }
        lock(&self.requests).push(req);
        Ok(PublishedCopyId::new())
    }
}

#[derive(Default)]
pub struct FakeFolders {
    pub labels: Mutex<HashMap<TeamFolderId, FolderLabel>>,
    pub adoptions: Mutex<HashMap<KnowledgePageId, AdoptionLabel>>,
}

#[async_trait::async_trait]
impl KnowledgeFolders for FakeFolders {
    async fn label_for(
        &self,
        _workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
        _item_path: &str,
    ) -> Option<FolderLabel> {
        lock(&self.labels).get(&folder_id).map(|l| FolderLabel {
            name: l.name.clone(),
            description: l.description.clone(),
            set_by_company: l.set_by_company,
        })
    }

    async fn adoption_for(
        &self,
        _workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
    ) -> Option<AdoptionLabel> {
        lock(&self.adoptions).get(&page_id).map(|a| AdoptionLabel {
            adopted_from: a.adopted_from.clone(),
            mark: a.mark,
        })
    }
}

#[derive(Default)]
pub struct FakeIdentity {
    pub login: Mutex<Option<String>>,
}

#[async_trait::async_trait]
impl KnowledgeIdentity for FakeIdentity {
    async fn git_login(&self) -> Option<String> {
        lock(&self.login).clone()
    }
}

// ---------------------------------------------------------------- agent profile

#[derive(Default)]
pub struct FakeSettings {
    pub hidden: Mutex<HashMap<ScopeId, Vec<String>>>,
    pub own: Mutex<HashMap<ScopeId, bool>>,
    pub numbers: Mutex<HashMap<ScopeId, i64>>,
    pub unavailable: Mutex<bool>,
}

#[async_trait::async_trait]
impl KnowledgeScopeSettings for FakeSettings {
    async fn number(&self, scope: ScopeId, _name: &str) -> Result<i64, SettingsReadError> {
        if *lock(&self.unavailable) {
            return Err(SettingsReadError::Unavailable);
        }
        lock(&self.numbers)
            .get(&scope)
            .copied()
            .ok_or(SettingsReadError::Unknown)
    }
    async fn flag(&self, scope: ScopeId, _name: &str) -> Result<bool, SettingsReadError> {
        if *lock(&self.unavailable) {
            return Err(SettingsReadError::Unavailable);
        }
        Ok(lock(&self.own).get(&scope).copied().unwrap_or(false))
    }
    async fn selection(
        &self,
        scope: ScopeId,
        _name: &str,
    ) -> Result<Vec<String>, SettingsReadError> {
        if *lock(&self.unavailable) {
            return Err(SettingsReadError::Unavailable);
        }
        Ok(lock(&self.hidden).get(&scope).cloned().unwrap_or_default())
    }
}

#[derive(Default)]
pub struct FakeMetrics {
    pub events: Mutex<Vec<(String, String, u64)>>,
}

impl FakeMetrics {
    pub fn count(&self, kind: &str, source: &str) -> u64 {
        lock(&self.events)
            .iter()
            .filter(|(k, s, _)| k == kind && s == source)
            .map(|(_, _, n)| *n)
            .sum()
    }
    pub fn of(&self, kind: &str) -> Vec<(String, String, u64)> {
        lock(&self.events)
            .iter()
            .filter(|(k, _, _)| k.starts_with(kind))
            .cloned()
            .collect()
    }
}

impl KnowledgeMetrics for FakeMetrics {
    fn query(&self, source: &str, operation: &'static str) {
        lock(&self.events).push((format!("query:{operation}"), source.to_owned(), 1));
    }
    fn note_written(&self, source: &str, operation: &'static str) {
        lock(&self.events).push((format!("note_written:{operation}"), source.to_owned(), 1));
    }
    fn note_refused(&self, source: &str, reason: &'static str) {
        lock(&self.events).push((format!("note_refused:{reason}"), source.to_owned(), 1));
    }
    fn items_written(&self, source: &str, count: u64) {
        lock(&self.events).push(("items_written".to_owned(), source.to_owned(), count));
    }
    fn ingest_thread_failed(&self, source: &str) {
        lock(&self.events).push(("ingest_thread_failed".to_owned(), source.to_owned(), 1));
    }
}

#[derive(Default)]
pub struct FakeRuns {
    pub questions: Mutex<HashMap<RunId, String>>,
    pub opened: Mutex<Vec<(RunId, uuid::Uuid)>>,
}

#[async_trait::async_trait]
impl KnowledgeRunContext for FakeRuns {
    async fn question(&self, run_id: RunId) -> Option<String> {
        lock(&self.questions).get(&run_id).cloned()
    }
    async fn item_opened(&self, run_id: RunId, item_id: uuid::Uuid) {
        let mut o = lock(&self.opened);
        if !o.contains(&(run_id, item_id)) {
            o.push((run_id, item_id));
        }
    }
}

#[derive(Default)]
pub struct FakeChat {
    pub chats: Mutex<HashMap<ScopeId, ScopeChat>>,
    /// cursor ("" for the first page) → page
    pub pages: Mutex<HashMap<String, ChatHistoryPage>>,
    /// (thread, cursor) → page
    pub replies: Mutex<HashMap<(String, String), ChatHistoryPage>>,
    pub failing_threads: Mutex<HashSet<String>>,
    pub unavailable: Mutex<bool>,
    pub reads: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl ChatHistory for FakeChat {
    async fn chat_of(&self, scope: ScopeId) -> Result<Option<ScopeChat>, ChatHistoryError> {
        Ok(lock(&self.chats).get(&scope).cloned())
    }
    async fn page(
        &self,
        _connection: &str,
        _chat: &str,
        _oldest: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<ChatHistoryPage, ChatHistoryError> {
        lock(&self.reads).push(format!("page:{}", cursor.unwrap_or("")));
        if *lock(&self.unavailable) {
            return Err(ChatHistoryError::Unavailable);
        }
        Ok(lock(&self.pages)
            .get(cursor.unwrap_or(""))
            .cloned()
            .unwrap_or(ChatHistoryPage {
                items: Vec::new(),
                newest_position: None,
                next_cursor: None,
                threads: Vec::new(),
            }))
    }
    async fn replies(
        &self,
        _connection: &str,
        _chat: &str,
        thread: &str,
        cursor: Option<&str>,
    ) -> Result<ChatHistoryPage, ChatHistoryError> {
        lock(&self.reads).push(format!("replies:{thread}"));
        if lock(&self.failing_threads).contains(thread) {
            return Err(ChatHistoryError::Unavailable);
        }
        Ok(lock(&self.replies)
            .get(&(thread.to_owned(), cursor.unwrap_or("").to_owned()))
            .cloned()
            .unwrap_or(ChatHistoryPage {
                items: Vec::new(),
                newest_position: None,
                next_cursor: None,
                threads: Vec::new(),
            }))
    }
}

/// Every fake of one service.
pub struct Fakes {
    pub files: Arc<FakeFiles>,
    pub workspaces: Arc<FakeWorkspaces>,
    pub rules: Arc<FakeRules>,
    pub memory: Arc<FakeMemory>,
    pub push: Arc<FakePush>,
    pub folders: Arc<FakeFolders>,
    pub identity: Arc<FakeIdentity>,
    pub settings: Arc<FakeSettings>,
    pub metrics: Arc<FakeMetrics>,
    pub runs: Arc<FakeRuns>,
    pub chat: Arc<FakeChat>,
}

impl Fakes {
    pub fn new() -> Fakes {
        Fakes {
            files: Arc::new(FakeFiles::new()),
            workspaces: Arc::new(FakeWorkspaces::default()),
            rules: Arc::new(FakeRules::default()),
            memory: Arc::new(FakeMemory::default()),
            push: Arc::new(FakePush::default()),
            folders: Arc::new(FakeFolders::default()),
            identity: Arc::new(FakeIdentity::default()),
            settings: Arc::new(FakeSettings::default()),
            metrics: Arc::new(FakeMetrics::default()),
            runs: Arc::new(FakeRuns::default()),
            chat: Arc::new(FakeChat::default()),
        }
    }

    pub fn deps(
        &self,
        journal: Arc<Journal>,
        agent: Option<(Vec<KnowledgeSourceConfig>, bool)>,
    ) -> KnowledgeDeps {
        KnowledgeDeps {
            journal,
            files: self.files.clone(),
            workspaces: self.workspaces.clone(),
            rules: self.rules.clone(),
            memory: self.memory.clone(),
            push: self.push.clone(),
            agent: agent.map(|(sources, note_tools)| AgentKnowledgeDeps {
                sources,
                settings: self.settings.clone(),
                metrics: self.metrics.clone(),
                runs: self.runs.clone(),
                chat_history: self.chat.clone(),
                note_tools,
            }),
            folders: self.folders.clone(),
            identity: self.identity.clone(),
            app_version: "0.61.0".to_owned(),
        }
    }
}

/// A migrated SQLite datastore on a temporary file and its journal.
pub async fn sqlite_journal(dir: &TempDir) -> (Datastore, Arc<Journal>) {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("journal.db"),
        backup_dir: dir.0.join("backups"),
    })
    .await
    .expect("the datastore");
    migrate(&store).await;
    (store.clone(), journal_on(store).await)
}

pub async fn migrate(store: &Datastore) {
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("the journal");
    registry
        .register(KnowledgeService::migrations())
        .expect("this module");
    registry
        .apply(store, ApplyMode::ApplyPending)
        .await
        .expect("the migrations");
}

pub async fn journal_on(store: Datastore) -> Arc<Journal> {
    Arc::new(
        Journal::start(
            store,
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(86_400),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("the journal"),
    )
}

/// The `knowledge_<name>` entries of the journal: (kind, data).
pub async fn journal_entries(store: &Datastore) -> Vec<(String, serde_json::Value)> {
    let pool = store.sqlite().expect("sqlite");
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT kind, data FROM journal_entries WHERE component = 'knowledge' ORDER BY recorded_at, id")
            .fetch_all(pool)
            .await
            .expect("journal rows");
    rows.into_iter()
        .map(|(k, d)| (k, serde_json::from_str(&d).unwrap_or_default()))
        .collect()
}

pub fn program(agent: bool, scaled: bool) -> ProgramKind {
    match (agent, scaled) {
        (false, _) => ProgramKind::Desktop,
        (true, false) => ProgramKind::AgentSingle,
        (true, true) => ProgramKind::AgentScaled,
    }
}
