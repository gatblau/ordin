//! The test rig: a real Journal on SQLite, local bare repositories as the test Git host, and fakes
//! of every trait this crate owns (the program's implementations are delegations, D3).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use git2::{Oid, Repository, Signature};
use ordin_identity::{CredError, PersonProfile};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_knowledge::{KnowledgeError, TeamPageFile, TeamPageImport, TeamPageImported};
use ordin_memory_service::{
    MemoryError, TeamEntity, TeamExportPage, TeamFact, TeamImportBatch, TeamImportOutcome,
    TeamImportResult, TeamRelationship, TeamTerm,
};
use ordin_projects::{BranchReviewRules, GitCredential, GithubError, RepoPermission};
use ordin_push::{PublishCopyRequest, PushError, PushNotice, TeamShareProposal};
use ordin_types::domain::{
    FolderAccess, FolderGrant, Grantee, PlaceKind, RunState, SourceLink, SourceLinkKind,
    SourceLinkState, WorkingCopyItem, WorkingCopyState, Workspace, WorkspaceState,
};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::AgentRepositoryRef;
use ordin_types::{
    AgentInstanceId, ApprovalId, FolderGrantId, KnowledgePageId, MemoryFactId, PlaceId,
    PublishedCopyId, RunId, SourceLinkId, TeamGrantId, TeamShareId, WorkingCopyItemId, WorkspaceId,
};
use ordin_workspaces::{
    GrantDecision, LinkLocation, NewPendingGrant, OriginReach, PendingGrant, PendingGrantState,
    Place, Reach, TeamResultFile, WorkspaceError,
};
use tokio::sync::broadcast;

use crate::ports::*;
use crate::service::TeamKnowledgeService;

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-tk-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        TempDir(dir)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn ts(raw: &str) -> Timestamp {
    serde_json::from_value(serde_json::Value::String(raw.to_owned())).expect("timestamp")
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------- the test Git host

pub const REPO: &str = "acme/supplier-team";
pub const FOLDER: &str = "knowledge";

/// A bare repository at `<base>/acme/supplier-team.git` with one commit on `main`.
pub struct Host {
    pub base: PathBuf,
}

impl Host {
    pub fn new(dir: &Path) -> Host {
        let base = dir.join("host");
        let path = base.join(format!("{REPO}.git"));
        std::fs::create_dir_all(&path).expect("host dir");
        let repo = Repository::init_bare(&path).expect("bare");
        let sig = Signature::now("Seed", "seed@example.org").expect("sig");
        let blob = repo.blob(b"seed\n").expect("blob");
        let mut builder = repo.treebuilder(None).expect("builder");
        builder.insert("README.md", blob, 0o100644).expect("insert");
        let tree = repo
            .find_tree(builder.write().expect("tree"))
            .expect("tree");
        repo.commit(Some("refs/heads/main"), &sig, &sig, "seed", &tree, &[])
            .expect("commit");
        repo.set_head("refs/heads/main").expect("head");
        Host { base }
    }

    pub fn repo(&self) -> Repository {
        Repository::open_bare(self.base.join(format!("{REPO}.git"))).expect("open host")
    }

    pub fn tip(&self) -> Oid {
        self.repo().refname_to_id("refs/heads/main").expect("main")
    }

    /// A colleague's commit on `main`: each path below the folder with its bytes, `None` deletes.
    pub fn commit(&self, files: &[(&str, Option<&[u8]>)], author: &str) -> Oid {
        let repo = self.repo();
        let parent = self.tip();
        let mut changes = Vec::new();
        for (p, bytes) in files {
            let oid = bytes.map(|b| repo.blob(b).expect("blob"));
            changes.push(((*p).to_owned(), oid));
        }
        let sig = Signature::now(author, "colleague@example.org").expect("sig");
        let commit =
            crate::git::commit(&repo, parent, FOLDER, &changes, &sig, "colleague").expect("commit");
        repo.reference("refs/heads/main", commit, true, "colleague")
            .expect("move main");
        commit
    }

    /// The file below the folder at the tip, if any.
    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        let repo = self.repo();
        let oid = crate::git::blob_at(&repo, self.tip(), FOLDER, path)?;
        let blob = repo.find_blob(oid).ok()?;
        let bytes = blob.content().to_vec();
        Some(bytes)
    }

    pub fn files(&self) -> BTreeMap<String, Oid> {
        crate::git::folder_files(&self.repo(), self.tip(), FOLDER).expect("files")
    }

    pub fn url(&self) -> String {
        format!("https://github.com/{REPO}")
    }
}

/// Pushes `commit` of the clone at `clone` to the host's `main` (what the Push Service does after
/// the card is approved).
pub fn push_to_host(clone: &Path, commit: &str, host: &Host, branch: &str) {
    let repo = Repository::open_bare(clone).expect("clone");
    let oid = Oid::from_str(commit).expect("oid");
    repo.reference("refs/ordin/share", oid, true, "share")
        .expect("ref");
    let mut remote = repo.find_remote("origin").expect("origin");
    remote
        .push(
            &[format!("+refs/ordin/share:refs/heads/{branch}").as_str()],
            None,
        )
        .expect("push");
    let _ = host;
}

// ---------------------------------------------------------------- Memory

#[derive(Default)]
pub struct MemoryState {
    pub facts: BTreeMap<String, TeamFact>,
    pub entities: BTreeMap<String, TeamEntity>,
    pub relationships: BTreeMap<String, TeamRelationship>,
    pub terms: BTreeMap<String, TeamTerm>,
    pub held: u32,
    pub imported: Vec<TeamImportBatch>,
    pub forgotten: Vec<MemoryFactId>,
    pub fail: Option<bool>,
    /// Term labels here with another kind of object (TK-TR-5).
    pub term_differs: HashSet<String>,
}

#[derive(Default)]
pub struct FakeMemory {
    pub state: Mutex<MemoryState>,
}

pub fn fact(statement: &str, entity: &str) -> TeamFact {
    let id = MemoryFactId::new();
    TeamFact {
        format: 2,
        id,
        memory_fact_id: format!("fact:{}", uuid::Uuid::new_v4()),
        statement: statement.to_owned(),
        about: None,
        memory_class: "episodic".to_owned(),
        fact_class: None,
        origin_kind: "document".to_owned(),
        source_id: None,
        entities: vec![entity.to_owned()],
        valid_from: ts("2026-01-10T09:12:44.318Z"),
        valid_to: None,
        confidence_permille: 1000,
        supersedes: None,
        superseded_by: None,
        derived_from: vec![],
        written_at: ts("2026-01-10T09:12:44.318Z"),
        shared_by: String::new(),
        shared_at: ts("2026-01-10T09:30:02.114Z"),
        triple: None,
        single_valued: false,
        changed: false,
    }
}

pub fn entity(name: &str) -> TeamEntity {
    TeamEntity {
        format: 1,
        id: format!("entity:{}", uuid::Uuid::new_v4()),
        canonical_name: name.to_owned(),
        aliases: vec![],
    }
}

impl FakeMemory {
    pub fn add(&self, f: TeamFact) -> TeamFact {
        lock(&self.state).facts.insert(f.id.to_string(), f.clone());
        f
    }
    pub fn add_entity(&self, e: TeamEntity) {
        lock(&self.state).entities.insert(e.id.clone(), e);
    }
    pub fn facts(&self) -> Vec<TeamFact> {
        lock(&self.state).facts.values().cloned().collect()
    }
}

fn mem_error(retryable: bool) -> MemoryError {
    MemoryError::Unavailable {
        code: "MEM_STORE_UNAVAILABLE",
        retryable,
    }
}

#[async_trait::async_trait]
impl TeamMemory for FakeMemory {
    async fn export_team(
        &self,
        _ws: WorkspaceId,
        _cursor: Option<String>,
    ) -> Result<TeamExportPage, MemoryError> {
        let s = lock(&self.state);
        if let Some(r) = s.fail {
            return Err(mem_error(r));
        }
        let named: HashSet<&String> = s.facts.values().flat_map(|f| f.entities.iter()).collect();
        let keys: HashSet<String> = s
            .facts
            .values()
            .filter_map(|f| f.triple.as_ref().map(|t| t.predicate.clone()))
            .collect();
        Ok(TeamExportPage {
            facts: s.facts.values().cloned().collect(),
            terms: s
                .terms
                .values()
                .filter(|t| keys.contains(&t.key))
                .cloned()
                .collect(),
            entities: s
                .entities
                .values()
                .filter(|e| named.contains(&e.id))
                .cloned()
                .collect(),
            sources: vec![],
            relationships: s.relationships.values().cloned().collect(),
            held_for_check: s.held,
            next_cursor: None,
        })
    }

    async fn import_team(
        &self,
        _ws: WorkspaceId,
        batch: TeamImportBatch,
    ) -> Result<Vec<TeamImportOutcome>, MemoryError> {
        let mut s = lock(&self.state);
        if let Some(r) = s.fail {
            return Err(mem_error(r));
        }
        s.imported.push(batch.clone());
        for e in &batch.entities {
            s.entities.insert(e.id.clone(), e.clone());
        }
        for r in &batch.relationships {
            s.relationships.insert(r.id.clone(), r.clone());
        }
        let mut out = Vec::new();
        for f in batch.facts {
            let refused = f.statement.contains("ignore previous instructions")
                || f.triple.as_ref().is_some_and(|t| {
                    batch
                        .terms
                        .iter()
                        .any(|term| term.key == t.predicate && s.term_differs.contains(&term.label))
                });
            if refused {
                out.push(TeamImportOutcome {
                    item_id: f.id.to_string(),
                    result: TeamImportResult::NotTaken {
                        code: "MEM_TEAM_GATE_REFUSED",
                    },
                });
                continue;
            }
            for t in batch.terms.iter() {
                s.terms.entry(t.key.clone()).or_insert_with(|| t.clone());
            }
            s.facts.insert(f.id.to_string(), f.clone());
            out.push(TeamImportOutcome {
                item_id: f.id.to_string(),
                result: TeamImportResult::Kept,
            });
        }
        Ok(out)
    }

    async fn forget_team_fact(
        &self,
        _ws: WorkspaceId,
        id: MemoryFactId,
    ) -> Result<(), MemoryError> {
        let mut s = lock(&self.state);
        s.facts.remove(&id.to_string());
        s.forgotten.push(id);
        Ok(())
    }

    async fn held_for_check(&self, _ws: WorkspaceId) -> Result<Vec<MemoryFactId>, MemoryError> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------- Pages

#[derive(Default)]
pub struct FakePages {
    pub pages: Mutex<BTreeMap<KnowledgePageId, Vec<u8>>>,
    pub removed: Mutex<Vec<KnowledgePageId>>,
    pub imports: Mutex<Vec<TeamPageImport>>,
    pub fail: Mutex<Option<bool>>,
}

/// A page file as the Knowledge Service writes it, `shared_by` empty until named (L-2).
pub fn page(id: KnowledgePageId, title: &str, body: &str, shared_by: &str) -> Vec<u8> {
    use ordin_knowledge::okf::{render_concept, OkfConcept, OrdinPageKeys};
    render_concept(&OkfConcept {
        path: "page.md".into(),
        type_: Some("Explanation".into()),
        ordin_page: true,
        title: title.into(),
        title_written: true,
        description: None,
        tags: vec![],
        aliases: vec![],
        links: vec![],
        sources: vec![],
        generated: None,
        verified: vec![],
        status: None,
        status_text: None,
        stale_after: None,
        sections: vec![],
        ordin: Some(OrdinPageKeys {
            page_id: id,
            written_by: ordin_types::domain::KnowledgeAuthor::Person,
            last_changed: ts("2026-01-10T09:00:00.000Z"),
            shared_by: Some(shared_by.into()),
            shared_at: Some(ts("2026-01-10T09:30:02.114Z")),
        }),
        extra_header: String::new(),
        body: body.into(),
    })
    .into_bytes()
}

#[async_trait::async_trait]
impl TeamPages for FakePages {
    async fn export_team_pages(
        &self,
        _ws: WorkspaceId,
    ) -> Result<Vec<TeamPageFile>, KnowledgeError> {
        if let Some(r) = *lock(&self.fail) {
            return Err(if r {
                KnowledgeError::DbUnavailable
            } else {
                KnowledgeError::WorkspaceNotFound
            });
        }
        Ok(lock(&self.pages)
            .iter()
            .map(|(id, bytes)| TeamPageFile {
                page_id: *id,
                bytes: bytes.clone(),
                last_changed: Timestamp::now(),
            })
            .collect())
    }
    async fn import_team_page(
        &self,
        _ws: WorkspaceId,
        page: TeamPageImport,
    ) -> Result<TeamPageImported, KnowledgeError> {
        lock(&self.imports).push(page.clone());
        if page.both_changed {
            return Ok(TeamPageImported::KeptLocal { remote_version: 2 });
        }
        lock(&self.pages).insert(page.page_id, page.bytes);
        Ok(TeamPageImported::Written { version: 1 })
    }
    async fn remove_team_page(
        &self,
        _ws: WorkspaceId,
        page_id: KnowledgePageId,
    ) -> Result<(), KnowledgeError> {
        lock(&self.pages).remove(&page_id);
        lock(&self.removed).push(page_id);
        Ok(())
    }
}

// ---------------------------------------------------------------- Push

pub struct FakePush {
    pub proposals: Mutex<Vec<TeamShareProposal>>,
    pub resent: Mutex<Vec<(TeamShareId, String)>>,
    pub withdrawn: Mutex<Vec<TeamShareId>>,
    pub published: Mutex<Vec<PublishCopyRequest>>,
    pub refuse: Mutex<Option<PushError>>,
    /// `publish_result` refused with this error, every time while set.
    pub refuse_publish: Mutex<Option<PushError>>,
    pub notices: broadcast::Sender<PushNotice>,
}

impl Default for FakePush {
    fn default() -> Self {
        FakePush {
            proposals: Mutex::new(vec![]),
            resent: Mutex::new(vec![]),
            withdrawn: Mutex::new(vec![]),
            published: Mutex::new(vec![]),
            refuse: Mutex::new(None),
            refuse_publish: Mutex::new(None),
            notices: broadcast::channel(64).0,
        }
    }
}

impl FakePush {
    pub fn last(&self) -> TeamShareProposal {
        lock(&self.proposals).last().cloned().expect("a proposal")
    }
    pub fn count(&self) -> usize {
        lock(&self.proposals).len()
    }
}

#[async_trait::async_trait]
impl TeamPush for FakePush {
    async fn propose_team_share(&self, p: TeamShareProposal) -> Result<ApprovalId, PushError> {
        if let Some(e) = lock(&self.refuse).take() {
            return Err(e);
        }
        lock(&self.proposals).push(p);
        Ok(ApprovalId::new())
    }
    async fn resend_team_share(
        &self,
        share_id: TeamShareId,
        commit: String,
    ) -> Result<(), PushError> {
        lock(&self.resent).push((share_id, commit));
        Ok(())
    }
    async fn withdraw_team_share(&self, share_id: TeamShareId) -> Result<(), PushError> {
        lock(&self.withdrawn).push(share_id);
        Ok(())
    }
    fn subscribe(&self) -> broadcast::Receiver<PushNotice> {
        self.notices.subscribe()
    }
    async fn publish_result(&self, req: PublishCopyRequest) -> Result<PublishedCopyId, PushError> {
        if let Some(e) = lock(&self.refuse_publish).as_ref() {
            return Err(PushError::new(e.code, e.detail.clone()));
        }
        lock(&self.published).push(req);
        Ok(PublishedCopyId::new())
    }
}

// ---------------------------------------------------------------- Workspaces

pub struct ResultFile {
    pub bytes: Vec<u8>,
    pub run: Option<RunId>,
    pub sent: bool,
}

#[derive(Default)]
pub struct WsState {
    pub workspaces: BTreeMap<WorkspaceId, Workspace>,
    pub links: Vec<SourceLink>,
    pub grants: Vec<FolderGrant>,
    pub results_folder: HashMap<WorkspaceId, PlaceId>,
    pub results: HashMap<WorkspaceId, BTreeMap<String, ResultFile>>,
    pub pending: Vec<PendingGrant>,
    pub reach: HashMap<String, Reach>,
    /// add_link_from_origin refused for these locations, with this error.
    pub link_refused: HashMap<String, WorkspaceError>,
    pub changed_copies: HashSet<SourceLinkId>,
    pub ended: Vec<WorkspaceId>,
    pub fail_set_grant: bool,
    pub removed_links: Vec<SourceLinkId>,
    pub removed_results: Vec<String>,
    /// What resolve_origin answers for an origin (WS-TW-6 step 3); absent → `WS_SOURCE_NOT_FOUND`.
    pub locations: HashMap<String, LinkLocation>,
    /// resolve_origin refused for these origins, with this error.
    pub resolve_refused: HashMap<String, WorkspaceError>,
}

pub struct FakeWorkspaces {
    pub state: Mutex<WsState>,
    pub opened: broadcast::Sender<WorkspaceId>,
    pub deleted: broadcast::Sender<WorkspaceId>,
    pub removed: broadcast::Sender<(WorkspaceId, SourceLinkId)>,
}

impl Default for FakeWorkspaces {
    fn default() -> Self {
        FakeWorkspaces {
            state: Mutex::new(WsState::default()),
            opened: broadcast::channel(16).0,
            deleted: broadcast::channel(16).0,
            removed: broadcast::channel(16).0,
        }
    }
}

impl FakeWorkspaces {
    pub fn workspace(&self, name: &str) -> WorkspaceId {
        let w = Workspace {
            id: WorkspaceId::new(),
            name: name.to_owned(),
            own_folder_place_id: PlaceId::new(),
            state: WorkspaceState::Active,
            last_seen_at: None,
            moved_to_bin_at: None,
            created_at: Timestamp::now(),
        };
        let id = w.id;
        lock(&self.state).workspaces.insert(id, w);
        id
    }

    pub fn link(
        &self,
        ws: WorkspaceId,
        kind: SourceLinkKind,
        origin: &str,
        name: &str,
    ) -> SourceLink {
        let place = (!matches!(kind, SourceLinkKind::GitRemote | SourceLinkKind::JiraIssue))
            .then(PlaceId::new);
        let link = SourceLink {
            id: SourceLinkId::new(),
            workspace_id: ws,
            kind,
            origin_location: origin.to_owned(),
            display_name: name.to_owned(),
            place_id: place,
            state: SourceLinkState::Active,
            added_at: Timestamp::now(),
            last_read_at: None,
        };
        lock(&self.state).links.push(link.clone());
        link
    }

    pub fn grant(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        place: PlaceId,
        access: FolderAccess,
    ) {
        let mut s = lock(&self.state);
        s.grants.retain(|g| {
            !(g.place_id == place
                && g.grantee
                    == Grantee::AgentInstance {
                        agent_instance_id: agent,
                    })
        });
        s.grants.push(FolderGrant {
            id: FolderGrantId::new(),
            workspace_id: ws,
            grantee: Grantee::AgentInstance {
                agent_instance_id: agent,
            },
            place_id: place,
            access,
            granted_at: Timestamp::now(),
        });
    }

    pub fn grants_of(&self, agent: AgentInstanceId) -> Vec<(PlaceId, FolderAccess)> {
        lock(&self.state)
            .grants
            .iter()
            .filter(|g| {
                g.grantee
                    == Grantee::AgentInstance {
                        agent_instance_id: agent,
                    }
            })
            .map(|g| (g.place_id, g.access))
            .collect()
    }

    pub fn write_result(&self, ws: WorkspaceId, path: &str, bytes: &[u8], run: Option<RunId>) {
        lock(&self.state).results.entry(ws).or_default().insert(
            path.to_owned(),
            ResultFile {
                bytes: bytes.to_vec(),
                run,
                sent: false,
            },
        );
    }

    pub fn result(&self, ws: WorkspaceId, path: &str) -> Option<Vec<u8>> {
        lock(&self.state)
            .results
            .get(&ws)
            .and_then(|r| r.get(path))
            .map(|f| f.bytes.clone())
    }

    pub fn links_of(&self, ws: WorkspaceId) -> Vec<SourceLink> {
        lock(&self.state)
            .links
            .iter()
            .filter(|l| l.workspace_id == ws)
            .cloned()
            .collect()
    }

    pub fn name_of(&self, ws: WorkspaceId) -> String {
        lock(&self.state).workspaces[&ws].name.clone()
    }
}

fn sha(bytes: &[u8]) -> Sha256Hex {
    Sha256Hex(crate::service::sha256_hex(bytes))
}

#[async_trait::async_trait]
impl TeamWorkspaces for FakeWorkspaces {
    async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        lock(&self.state)
            .workspaces
            .get(&id)
            .cloned()
            .ok_or(WorkspaceError::NotFound)
    }
    async fn source_links(&self, id: WorkspaceId) -> Result<Vec<SourceLink>, WorkspaceError> {
        Ok(self.links_of(id))
    }
    async fn add_link(
        &self,
        id: WorkspaceId,
        location: LinkLocation,
        display_name: Option<String>,
    ) -> Result<SourceLink, WorkspaceError> {
        match location {
            LinkLocation::GitRemote { url, branch, path } => {
                if !url.starts_with("https://") {
                    return Err(WorkspaceError::GitRemoteInvalid);
                }
                let url = url.trim_end_matches(".git").to_owned();
                Ok(self.link(
                    id,
                    SourceLinkKind::GitRemote,
                    &format!("git:{url}#{branch}:{path}"),
                    &display_name.unwrap_or_else(|| path.clone()),
                ))
            }
            _ => Err(WorkspaceError::OriginInvalid),
        }
    }
    fn opened(&self) -> broadcast::Receiver<WorkspaceId> {
        self.opened.subscribe()
    }
    fn deleted(&self) -> broadcast::Receiver<WorkspaceId> {
        self.deleted.subscribe()
    }
    fn links_removed(&self) -> broadcast::Receiver<(WorkspaceId, SourceLinkId)> {
        self.removed.subscribe()
    }
    async fn create(&self, name: String) -> Result<Workspace, WorkspaceError> {
        let taken = lock(&self.state)
            .workspaces
            .values()
            .any(|w| w.name.eq_ignore_ascii_case(&name) && w.state != WorkspaceState::InBin);
        if taken {
            return Err(WorkspaceError::Invalid("WS_NAME_DUPLICATE"));
        }
        let id = self.workspace(&name);
        self.get(id).await
    }
    async fn rename(&self, id: WorkspaceId, name: String) -> Result<Workspace, WorkspaceError> {
        let taken = lock(&self.state)
            .workspaces
            .values()
            .any(|w| w.id != id && w.name.eq_ignore_ascii_case(&name));
        if taken {
            return Err(WorkspaceError::Invalid("WS_NAME_DUPLICATE"));
        }
        let mut s = lock(&self.state);
        let w = s.workspaces.get_mut(&id).ok_or(WorkspaceError::NotFound)?;
        w.name = name;
        Ok(w.clone())
    }
    async fn move_to_bin(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        let mut s = lock(&self.state);
        let w = s.workspaces.get_mut(&id).ok_or(WorkspaceError::NotFound)?;
        w.state = WorkspaceState::InBin;
        Ok(w.clone())
    }
    async fn delete_from_bin(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        lock(&self.state).workspaces.remove(&id);
        Ok(())
    }
    async fn remove_link(&self, link: SourceLinkId) -> Result<(), WorkspaceError> {
        let mut s = lock(&self.state);
        let place = s
            .links
            .iter()
            .find(|l| l.id == link)
            .and_then(|l| l.place_id);
        s.links.retain(|l| l.id != link);
        s.grants.retain(|g| Some(g.place_id) != place);
        s.removed_links.push(link);
        Ok(())
    }
    async fn working_copy_items(
        &self,
        link: SourceLinkId,
    ) -> Result<Vec<WorkingCopyItem>, WorkspaceError> {
        let changed = lock(&self.state).changed_copies.contains(&link);
        Ok(if changed {
            vec![WorkingCopyItem {
                id: WorkingCopyItemId::new(),
                source_link_id: link,
                origin_path: "Contract.docx".to_owned(),
                origin_version_read: "1".to_owned(),
                local_version: 2,
                state: WorkingCopyState::Changed,
                read_at: Timestamp::now(),
                origin_newer_at: None,
            }]
        } else {
            vec![]
        })
    }
    async fn grants(&self, id: WorkspaceId) -> Result<Vec<FolderGrant>, WorkspaceError> {
        Ok(lock(&self.state)
            .grants
            .iter()
            .filter(|g| g.workspace_id == id)
            .cloned()
            .collect())
    }
    async fn set_grant(
        &self,
        id: WorkspaceId,
        grantee: Grantee,
        place_id: PlaceId,
        access: Option<FolderAccess>,
    ) -> Result<Option<FolderGrant>, WorkspaceError> {
        if lock(&self.state).fail_set_grant {
            return Err(WorkspaceError::GrantPublishFailed);
        }
        let Grantee::AgentInstance { agent_instance_id } = grantee else {
            return Ok(None);
        };
        match access {
            Some(a) => {
                self.grant(id, agent_instance_id, place_id, a);
            }
            None => lock(&self.state)
                .grants
                .retain(|g| !(g.place_id == place_id && g.grantee == grantee)),
        }
        Ok(None)
    }
    async fn check_origin(
        &self,
        _kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<OriginReach, WorkspaceError> {
        Ok(OriginReach {
            reach: lock(&self.state)
                .reach
                .get(origin_location)
                .copied()
                .unwrap_or(Reach::Reachable),
        })
    }
    async fn add_link_from_origin(
        &self,
        id: WorkspaceId,
        kind: SourceLinkKind,
        origin_location: &str,
        display_name: &str,
    ) -> Result<SourceLink, WorkspaceError> {
        if let Some(e) = lock(&self.state).link_refused.get(origin_location) {
            return Err(e.clone());
        }
        if let Some(l) = self
            .links_of(id)
            .into_iter()
            .find(|l| l.kind == kind && l.origin_location == origin_location)
        {
            return Ok(l);
        }
        Ok(self.link(id, kind, origin_location, display_name))
    }
    async fn make_results_folder(&self, id: WorkspaceId) -> Result<Place, WorkspaceError> {
        let place = {
            let mut s = lock(&self.state);
            match s.results_folder.get(&id) {
                Some(p) => *p,
                None => {
                    let p = PlaceId::new();
                    s.results_folder.insert(id, p);
                    p
                }
            }
        };
        Ok(Place {
            id: place,
            friendly_name: "Team results".into(),
            path: format!("/docs/ordin/Linked copies/results-{id}"),
            kind: PlaceKind::WorkingCopy,
            added_at: Timestamp::now(),
        })
    }
    async fn results_folder(&self, id: WorkspaceId) -> Result<Option<PlaceId>, WorkspaceError> {
        let s = lock(&self.state);
        Ok(if s.ended.contains(&id) {
            None
        } else {
            s.results_folder.get(&id).copied()
        })
    }
    async fn results_of_run(
        &self,
        id: WorkspaceId,
        run_id: RunId,
    ) -> Result<Vec<String>, WorkspaceError> {
        Ok(lock(&self.state)
            .results
            .get(&id)
            .map(|r| {
                r.iter()
                    .filter(|(_, f)| f.run == Some(run_id))
                    .map(|(p, _)| p.clone())
                    .collect()
            })
            .unwrap_or_default())
    }
    async fn unsent_results(&self, id: WorkspaceId) -> Result<Vec<TeamResultFile>, WorkspaceError> {
        Ok(lock(&self.state)
            .results
            .get(&id)
            .map(|r| {
                r.iter()
                    .filter(|(_, f)| !f.sent)
                    .map(|(p, f)| TeamResultFile {
                        path: p.clone(),
                        run_id: f.run,
                        version: 1,
                        sha256: sha(&f.bytes),
                        size_bytes: f.bytes.len() as u64,
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
    async fn read_result(&self, id: WorkspaceId, path: &str) -> Result<Vec<u8>, WorkspaceError> {
        self.result(id, path).ok_or(WorkspaceError::FilesFailed)
    }
    async fn store_team_result(
        &self,
        id: WorkspaceId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Sha256Hex, WorkspaceError> {
        ordin_workspaces::check_result_path(path)?;
        let h = sha(&bytes);
        lock(&self.state).results.entry(id).or_default().insert(
            path.to_owned(),
            ResultFile {
                bytes,
                run: None,
                sent: true,
            },
        );
        Ok(h)
    }
    async fn remove_team_result(&self, id: WorkspaceId, path: &str) -> Result<(), WorkspaceError> {
        let mut s = lock(&self.state);
        s.results.entry(id).or_default().remove(path);
        s.removed_results.push(path.to_owned());
        Ok(())
    }
    async fn mark_results_sent(
        &self,
        id: WorkspaceId,
        sent: Vec<(String, Sha256Hex)>,
    ) -> Result<(), WorkspaceError> {
        let mut s = lock(&self.state);
        for (p, _) in sent {
            if let Some(f) = s.results.entry(id).or_default().get_mut(&p) {
                f.sent = true;
            }
        }
        Ok(())
    }
    async fn keep_both_results(
        &self,
        id: WorkspaceId,
        path: &str,
        theirs: Vec<u8>,
        mine_to: &str,
    ) -> Result<Sha256Hex, WorkspaceError> {
        let mut s = lock(&self.state);
        let folder = s.results.entry(id).or_default();
        if folder.contains_key(mine_to) {
            return Err(WorkspaceError::ResultPathTaken);
        }
        let mine = folder.remove(path).ok_or(WorkspaceError::FilesFailed)?;
        folder.insert(
            mine_to.to_owned(),
            ResultFile {
                bytes: mine.bytes,
                run: mine.run,
                sent: false,
            },
        );
        let h = sha(&theirs);
        folder.insert(
            path.to_owned(),
            ResultFile {
                bytes: theirs,
                run: None,
                sent: true,
            },
        );
        Ok(h)
    }
    async fn record_pending_grant(
        &self,
        g: NewPendingGrant,
    ) -> Result<PendingGrant, WorkspaceError> {
        let mut s = lock(&self.state);
        for p in s.pending.iter_mut().filter(|p| {
            p.grant.workspace_id == g.workspace_id
                && p.grant.agent_key == g.agent_key
                && p.state == PendingGrantState::Waiting
        }) {
            p.state = PendingGrantState::Replaced;
        }
        let grant = PendingGrant {
            id: TeamGrantId::new(),
            grant: g,
            state: PendingGrantState::Waiting,
            decided_at: None,
        };
        s.pending.push(grant.clone());
        Ok(grant)
    }
    async fn pending_grants(&self, id: WorkspaceId) -> Result<Vec<PendingGrant>, WorkspaceError> {
        Ok(lock(&self.state)
            .pending
            .iter()
            .filter(|p| p.grant.workspace_id == id && p.state == PendingGrantState::Waiting)
            .cloned()
            .collect())
    }
    async fn decide_pending_grant(
        &self,
        grant: TeamGrantId,
        decision: GrantDecision,
    ) -> Result<PendingGrant, WorkspaceError> {
        let mut s = lock(&self.state);
        let p = s
            .pending
            .iter_mut()
            .find(|p| p.id == grant && p.state == PendingGrantState::Waiting)
            .ok_or(WorkspaceError::GrantNotFound)?;
        p.state = if decision == GrantDecision::Allowed {
            PendingGrantState::Allowed
        } else {
            PendingGrantState::Declined
        };
        p.decided_at = Some(Timestamp::now());
        Ok(p.clone())
    }
    async fn end_team(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let mut s = lock(&self.state);
        for p in s
            .pending
            .iter_mut()
            .filter(|p| p.grant.workspace_id == id && p.state == PendingGrantState::Waiting)
        {
            p.state = PendingGrantState::Declined;
        }
        s.ended.push(id);
        Ok(())
    }
    async fn resolve_origin(
        &self,
        _kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<LinkLocation, WorkspaceError> {
        let s = lock(&self.state);
        if let Some(e) = s.resolve_refused.get(origin_location) {
            return Err(e.clone());
        }
        s.locations
            .get(origin_location)
            .cloned()
            .ok_or(WorkspaceError::SourceNotFound)
    }
}

// ---------------------------------------------------------------- Repos

pub struct FakeRepos {
    pub permission: Mutex<Result<RepoPermission, GithubError>>,
    pub review_required: Mutex<bool>,
    pub person: Mutex<Option<PersonProfile>>,
}

impl Default for FakeRepos {
    fn default() -> Self {
        FakeRepos {
            permission: Mutex::new(Ok(RepoPermission::Write)),
            review_required: Mutex::new(false),
            person: Mutex::new(Some(PersonProfile {
                name: "Asha Patel".into(),
                email: Some("asha@example.org".into()),
                github_login: Some("asha-patel".into()),
            })),
        }
    }
}

#[async_trait::async_trait]
impl TeamRepos for FakeRepos {
    async fn git_credentials(&self, _host: &str) -> Result<GitCredential, GithubError> {
        Err(GithubError::SignInNeeded)
    }
    async fn repository_permission(
        &self,
        _host: &str,
        _repository: &str,
    ) -> Result<RepoPermission, GithubError> {
        lock(&self.permission).clone()
    }
    async fn branch_review_rules(
        &self,
        _h: &str,
        _r: &str,
        _b: &str,
    ) -> Result<BranchReviewRules, GithubError> {
        Ok(BranchReviewRules {
            review_required: *lock(&self.review_required),
            code_owner_review_required: false,
            required_approvals: 0,
        })
    }
    async fn person(&self) -> Result<PersonProfile, CredError> {
        lock(&self.person).clone().ok_or(CredError::Permanent {
            code: "CRED_NOT_FOUND",
        })
    }
}

// ---------------------------------------------------------------- Agents and runs

#[derive(Default)]
pub struct FakeAgents {
    /// Agents in each workspace: (instance, name, identity).
    pub here: Mutex<HashMap<WorkspaceId, Vec<TeamAgentHere>>>,
    /// Agents in this person's repositories, by id.
    pub library: Mutex<HashSet<String>>,
    pub added: Mutex<Vec<(WorkspaceId, String)>>,
    pub removed: Mutex<Vec<AgentInstanceId>>,
}

pub fn repository() -> AgentRepositoryRef {
    AgentRepositoryRef::ProfileKey("acme-agents".into())
}

impl FakeAgents {
    pub fn put(&self, ws: WorkspaceId, name: &str, id: &str) -> AgentInstanceId {
        let a = AgentInstanceId::new();
        lock(&self.here).entry(ws).or_default().push(TeamAgentHere {
            agent_instance_id: a,
            display_name: name.into(),
            identity: Some((repository(), id.into(), "1.3.0".into())),
        });
        a
    }
    pub fn in_workspace(&self, ws: WorkspaceId, id: &str) -> Option<AgentInstanceId> {
        lock(&self.here).get(&ws).and_then(|v| {
            v.iter()
                .find(|a| a.identity.as_ref().is_some_and(|i| i.1 == id))
                .map(|a| a.agent_instance_id)
        })
    }
}

#[async_trait::async_trait]
impl TeamAgents for FakeAgents {
    async fn agents_in_workspace(
        &self,
        ws: WorkspaceId,
    ) -> Result<Vec<TeamAgentHere>, TeamAgentError> {
        Ok(lock(&self.here).get(&ws).cloned().unwrap_or_default())
    }
    async fn find_agent(
        &self,
        ws: WorkspaceId,
        _r: &AgentRepositoryRef,
        id: &str,
        _v: &str,
    ) -> Result<TeamAgentFind, TeamAgentError> {
        if let Some(a) = self.in_workspace(ws, id) {
            return Ok(TeamAgentFind::Present {
                agent_instance_id: a,
            });
        }
        Ok(if lock(&self.library).contains(id) {
            TeamAgentFind::Available
        } else {
            TeamAgentFind::NotInYourRepositories
        })
    }
    async fn add_agent(
        &self,
        ws: WorkspaceId,
        _r: &AgentRepositoryRef,
        id: &str,
        _v: &str,
    ) -> Result<AgentInstanceId, TeamAgentError> {
        if !lock(&self.library).contains(id) {
            return Err(TeamAgentError {
                code: crate::TEAM_WS_AGENT_UNSAFE,
                retryable: false,
            });
        }
        lock(&self.added).push((ws, id.to_owned()));
        Ok(self.put(ws, id, id))
    }
    async fn remove_agent(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<(), TeamAgentError> {
        if let Some(v) = lock(&self.here).get_mut(&ws) {
            v.retain(|a| a.agent_instance_id != agent);
        }
        lock(&self.removed).push(agent);
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeRuns {
    pub runs: Mutex<HashMap<RunId, TeamRunFacts>>,
}

#[async_trait::async_trait]
impl TeamRuns for FakeRuns {
    async fn ended_task(&self, run_id: RunId) -> Result<Option<TeamRunFacts>, TeamRunsError> {
        Ok(lock(&self.runs).get(&run_id).cloned())
    }
}

pub fn run_facts(ws: WorkspaceId, agent: AgentInstanceId, task: &str) -> TeamRunFacts {
    TeamRunFacts {
        workspace_id: ws,
        agent_instance_id: agent,
        agent_name: "Supplier analyst".into(),
        task_title: Some(task.into()),
        state: RunState::Completed,
        ended_at: Timestamp::now(),
    }
}

// ---------------------------------------------------------------- the rig

pub struct Rig {
    pub dir: TempDir,
    pub host: Host,
    pub journal: Arc<Journal>,
    pub memory: Arc<FakeMemory>,
    pub pages: Arc<FakePages>,
    pub push: Arc<FakePush>,
    pub workspaces: Arc<FakeWorkspaces>,
    pub repos: Arc<FakeRepos>,
    pub agents: Arc<FakeAgents>,
    pub runs: Arc<FakeRuns>,
    pub service: Arc<TeamKnowledgeService>,
    pub ws: WorkspaceId,
    pub link: SourceLink,
}

pub async fn journal_in(dir: &Path) -> Arc<Journal> {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.join("journal.db"),
        backup_dir: dir.join("backups"),
    })
    .await
    .expect("datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry
        .register(TeamKnowledgeService::migrations())
        .expect("team knowledge");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("migrate");
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
        .expect("journal"),
    )
}

impl Rig {
    pub async fn new() -> Rig {
        let dir = TempDir::new();
        let host = Host::new(dir.path());
        let journal = journal_in(dir.path()).await;
        let memory = Arc::new(FakeMemory::default());
        let pages = Arc::new(FakePages::default());
        let push = Arc::new(FakePush::default());
        let workspaces = Arc::new(FakeWorkspaces::default());
        let repos = Arc::new(FakeRepos::default());
        let agents = Arc::new(FakeAgents::default());
        let runs = Arc::new(FakeRuns::default());
        let service = TeamKnowledgeService::new(TeamKnowledgeDeps {
            journal: journal.clone(),
            memory: memory.clone(),
            pages: pages.clone(),
            push: push.clone(),
            workspaces: workspaces.clone(),
            repos: repos.clone(),
            agents: agents.clone(),
            runs: runs.clone(),
            clones_dir: dir.path().join("data/team-knowledge"),
        });
        {
            let mut seams = crate::service::lock(&service.seams);
            seams.remote_base = Some(host.base.clone());
            seams.login = Some("asha-patel".into());
            seams.no_waits = true;
        }
        let ws = workspaces.workspace("Q3 supplier review");
        let link = workspaces.link(
            ws,
            SourceLinkKind::GitRemote,
            &format!("git:{}#main:{FOLDER}", host.url()),
            "knowledge",
        );
        Rig {
            dir,
            host,
            journal,
            memory,
            pages,
            push,
            workspaces,
            repos,
            agents,
            runs,
            service,
            ws,
            link,
        }
    }

    /// A second installation reading the same host (a colleague's computer).
    pub async fn colleague(&self, login: &str, name: &str) -> Rig {
        let dir = TempDir::new();
        let journal = journal_in(dir.path()).await;
        let memory = Arc::new(FakeMemory::default());
        let pages = Arc::new(FakePages::default());
        let push = Arc::new(FakePush::default());
        let workspaces = Arc::new(FakeWorkspaces::default());
        let repos = Arc::new(FakeRepos::default());
        *lock(&repos.person) = Some(PersonProfile {
            name: name.into(),
            email: None,
            github_login: Some(login.into()),
        });
        let agents = Arc::new(FakeAgents::default());
        let runs = Arc::new(FakeRuns::default());
        let service = TeamKnowledgeService::new(TeamKnowledgeDeps {
            journal: journal.clone(),
            memory: memory.clone(),
            pages: pages.clone(),
            push: push.clone(),
            workspaces: workspaces.clone(),
            repos: repos.clone(),
            agents: agents.clone(),
            runs: runs.clone(),
            clones_dir: dir.path().join("data/team-knowledge"),
        });
        {
            let mut seams = crate::service::lock(&service.seams);
            seams.remote_base = Some(self.host.base.clone());
            seams.login = Some(login.into());
            seams.no_waits = true;
        }
        let ws = workspaces.workspace("Q3 supplier review");
        let link = workspaces.link(
            ws,
            SourceLinkKind::GitRemote,
            &format!("git:{}#main:{FOLDER}", self.host.url()),
            "knowledge",
        );
        Rig {
            dir,
            host: Host {
                base: self.host.base.clone(),
            },
            journal,
            memory,
            pages,
            push,
            workspaces,
            repos,
            agents,
            runs,
            service,
            ws,
            link,
        }
    }

    pub fn choice(&self) -> crate::TeamFolderChoice {
        crate::TeamFolderChoice::Link {
            source_link_id: self.link.id,
        }
    }

    pub async fn on(&self) {
        self.service
            .turn_on(self.ws, self.choice())
            .await
            .expect("turn on");
    }

    /// Runs a share under the workspace's lock, as the long command does, and returns the proposal.
    pub async fn share(&self) -> Result<TeamShareProposal, crate::TeamError> {
        self.share_scope(crate::store::ShareScope::All).await
    }

    pub async fn share_scope(
        &self,
        scope: crate::store::ShareScope,
    ) -> Result<TeamShareProposal, crate::TeamError> {
        let guard = self.service.workspace_lock(self.ws);
        let _held = guard.lock().await;
        self.service.share_now(self.ws, scope).await?;
        Ok(self.push.last())
    }

    /// The card is approved and the Push Service sends the commit.
    pub async fn approve(&self, p: &TeamShareProposal) {
        push_to_host(&p.clone_path, &p.commit, &self.host, &p.branch);
        self.service
            .on_notice(PushNotice::TeamShareSent {
                share_id: p.share_id,
                commit: p.commit.clone(),
                pull_request: None,
            })
            .await;
    }

    pub async fn get(&self) -> Result<crate::GetReport, crate::TeamError> {
        let guard = self.service.workspace_lock(self.ws);
        let _held = guard.lock().await;
        self.service.get_now(self.ws).await.map(|(r, _)| r)
    }

    pub async fn check(&self) -> Result<(), crate::TeamError> {
        let guard = self.service.workspace_lock(self.ws);
        let _held = guard.lock().await;
        self.service.check(self.ws).await
    }

    pub async fn status(&self) -> crate::TeamKnowledgeStatus {
        self.service.status(self.ws).await.expect("status")
    }

    pub async fn journal_kinds(&self) -> Vec<String> {
        let pool = self.journal.datastore().sqlite().expect("sqlite").clone();
        sqlx::query_scalar("SELECT kind FROM journal_entries")
            .fetch_all(&pool)
            .await
            .expect("entries")
    }

    pub async fn wait_idle(&self) {
        for _ in 0..400 {
            if self.service.busy(self.ws).is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the long command did not end");
    }
}

pub fn changed_paths(host: &Host, commit: Oid) -> Vec<(String, bool)> {
    let repo = host.repo();
    let c = repo.find_commit(commit).expect("commit");
    let parent = c.parent(0).expect("parent");
    let diff = repo
        .diff_tree_to_tree(
            Some(&parent.tree().unwrap()),
            Some(&c.tree().unwrap()),
            None,
        )
        .expect("diff");
    diff.deltas()
        .map(|d| {
            let path = d
                .new_file()
                .path()
                .or_else(|| d.old_file().path())
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (path, d.status() == git2::Delta::Deleted)
        })
        .collect()
}
