//! The program's implementations of the seven ports the Knowledge Service of row 3.38 takes in
//! `KnowledgeDeps` (OrdinDesktop.md *Program-side implementations and calls*; KnowledgeService.md
//! SA-RECON-08, SA-RECON-09, SA-KBUILT-01). Each one delegates to the module the trait names and adds
//! no decision. The Package Catalogue (row 3.40) and the Crew Orchestrator are reached through
//! [`Late`].

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use ordin_agent_folders::AgentFolderManager;
use ordin_files::{
    BinTicket, Cursor, FileBytes, FileEntry, FileError, FileService, FileVersion, FolderSort, Page,
    PlaceChange, WriteVersion,
};
use ordin_identity::IdentityService;
use ordin_knowledge::{
    AgentKnowledgeFile, KnowledgeFiles, KnowledgeFolders, KnowledgeIdentity, KnowledgeMemory,
    KnowledgePush, KnowledgeRules, KnowledgeWorkspaces, PackageError, RulesUnavailable,
    SourceReadCheck, WorkspaceAgent,
};
use ordin_memory_service::{
    AdoptionLabel, ContentCheck, FolderLabel, MemoryError, MemoryService, PageGraph, PageSection,
};
use ordin_push::{PublishCopyRequest, PushError, PushService};
use ordin_rules::{PlaceRead, PlaceReader, RulesEngine};
use ordin_types::domain::places::{SourceLinkKind, Workspace};
use ordin_types::session::ToolCaller;
use ordin_types::{
    AgentInstanceId, CodedError, KnowledgePageId, PlaceId, PublishedCopyId, TeamFolderId,
    WorkspaceId,
};
use ordin_workspaces::{
    OriginChanged, OriginVersion, WorkspaceError, WorkspaceListFilter, WorkspaceService,
};

use crate::hosts::Late;

/// The agent's own knowledge folder, inside its agent folder.
const KNOWLEDGE_DIR: &str = "knowledge";
/// At most 1 MiB per knowledge file (SA-RECON-08).
const KNOWLEDGE_FILE_MAX: u64 = 1024 * 1024;
/// The Package Catalogue's code while it is not built yet (row 3.40 follows row 3.38).
const PACKAGES_NOT_BUILT: &str = "PKG_STORE_FAILED";

// ---------------------------------------------------------------- KnowledgeFiles

/// `KnowledgeFiles` over the File Service, and the agent's `knowledge/` folder over the Agent Folder
/// Manager.
pub struct DesktopKnowledgeFiles {
    pub files: Arc<FileService>,
    pub agent_folders: Arc<AgentFolderManager>,
}

impl DesktopKnowledgeFiles {
    async fn knowledge_root(&self, agent: AgentInstanceId) -> Result<PathBuf, FileError> {
        let folder = self
            .agent_folders
            .get(agent)
            .await
            .map_err(|_| FileError::NotFound)?;
        Ok(folder.folder_path.join(KNOWLEDGE_DIR))
    }
}

/// A Markdown file name.
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Every Markdown file of at most 1 MiB under `root`, as `/`-separated paths relative to it.
/// Symbolic links are not followed. A missing folder holds no file.
fn walk_knowledge(root: &Path) -> Result<Vec<AgentKnowledgeFile>, FileError> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && dir == root => {
                return Ok(Vec::new())
            }
            Err(_) => return Err(FileError::FolderUnreadable),
        };
        for entry in entries {
            let entry = entry.map_err(|_| FileError::FolderUnreadable)?;
            let kind = entry.file_type().map_err(|_| FileError::FolderUnreadable)?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && is_markdown(&path) {
                let bytes = entry
                    .metadata()
                    .map_err(|_| FileError::FolderUnreadable)?
                    .len();
                if bytes > KNOWLEDGE_FILE_MAX {
                    continue;
                }
                let Ok(relative) = path.strip_prefix(root) else {
                    continue;
                };
                let parts: Vec<String> = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.push(AgentKnowledgeFile {
                    relative_path: parts.join("/"),
                    bytes,
                });
            }
        }
    }
    found.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(found)
}

#[async_trait::async_trait]
impl KnowledgeFiles for DesktopKnowledgeFiles {
    async fn write_version(&self, w: WriteVersion) -> Result<FileVersion, FileError> {
        self.files.write_version(w).await
    }

    async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError> {
        self.files.read_file(place_id, path).await
    }

    async fn current_version(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<Option<FileVersion>, FileError> {
        self.files.current_version(place_id, path).await
    }

    async fn move_to_bin(&self, place_id: PlaceId, path: &str) -> Result<BinTicket, FileError> {
        self.files.move_to_bin(place_id, path).await
    }

    async fn restore_from_bin(&self, ticket: &BinTicket) -> Result<FileVersion, FileError> {
        self.files.restore_from_bin(ticket).await
    }

    fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<PlaceChange> {
        self.files.subscribe_changes()
    }

    async fn list_agent_knowledge(
        &self,
        agent: AgentInstanceId,
    ) -> Result<Vec<AgentKnowledgeFile>, FileError> {
        let root = self.knowledge_root(agent).await?;
        tokio::task::spawn_blocking(move || walk_knowledge(&root))
            .await
            .map_err(|_| FileError::FolderUnreadable)?
    }

    async fn read_agent_knowledge(
        &self,
        agent: AgentInstanceId,
        relative_path: &str,
    ) -> Result<Vec<u8>, FileError> {
        let relative = Path::new(relative_path);
        let plain = !relative_path.is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !plain {
            return Err(FileError::PathEscapesPlace);
        }
        if !is_markdown(relative) {
            return Err(FileError::NotFound);
        }
        let at = self.knowledge_root(agent).await?.join(relative);
        let meta = match tokio::fs::symlink_metadata(&at).await {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(FileError::NotFound),
            Err(_) => return Err(FileError::FolderUnreadable),
        };
        if !meta.is_file() {
            return Err(FileError::NotFound);
        }
        if meta.len() > KNOWLEDGE_FILE_MAX {
            return Err(FileError::TooLarge {
                limit_bytes: KNOWLEDGE_FILE_MAX,
            });
        }
        tokio::fs::read(&at)
            .await
            .map_err(|_| FileError::FolderUnreadable)
    }

    async fn move_file(
        &self,
        place_id: PlaceId,
        from: &str,
        to: &str,
    ) -> Result<FileVersion, FileError> {
        self.files.move_file(place_id, from, to).await
    }

    async fn versions(&self, place_id: PlaceId, path: &str) -> Result<Vec<FileVersion>, FileError> {
        self.files.versions(place_id, path).await
    }

    async fn list_folder(
        &self,
        place_id: PlaceId,
        folder: &str,
        sort: FolderSort,
        cursor: Option<Cursor>,
    ) -> Result<Page<FileEntry>, FileError> {
        self.files.list_folder(place_id, folder, sort, cursor).await
    }
}

// ---------------------------------------------------------------- KnowledgeWorkspaces

/// `KnowledgeWorkspaces` over the Workspace Service, and `agents_in_workspace` over the Package
/// Catalogue (*late-bound*: row 3.40).
pub struct DesktopKnowledgeWorkspaces {
    pub workspaces: Arc<WorkspaceService>,
    pub late: Arc<Late>,
}

#[async_trait::async_trait]
impl KnowledgeWorkspaces for DesktopKnowledgeWorkspaces {
    async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        self.workspaces.get(id).await
    }

    async fn list_not_in_bin(&self) -> Result<Vec<Workspace>, WorkspaceError> {
        self.workspaces.list(WorkspaceListFilter::NotInBin).await
    }

    async fn origin_version(
        &self,
        workspace_id: WorkspaceId,
        link_kind: SourceLinkKind,
        origin_path: &str,
    ) -> Result<Option<OriginVersion>, WorkspaceError> {
        self.workspaces
            .origin_version(workspace_id, link_kind, origin_path)
            .await
    }

    fn subscribe_origin_changes(&self) -> tokio::sync::broadcast::Receiver<OriginChanged> {
        self.workspaces.subscribe_origin_changes()
    }

    async fn agents_in_workspace(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<WorkspaceAgent>, PackageError> {
        let packages = self.late.packages.get().ok_or(PackageError {
            code: PACKAGES_NOT_BUILT,
        })?;
        let agents = packages
            .agents_in_workspace(id)
            .await
            .map_err(|e| PackageError { code: e.code() })?;
        Ok(agents
            .into_iter()
            .map(|a| WorkspaceAgent {
                agent_instance_id: a.agent_instance_id,
                display_name: a.display_name,
            })
            .collect())
    }
}

// ---------------------------------------------------------------- KnowledgeRules

/// `KnowledgeRules::may_read_source` (SA-RECON-09): the source is resolved to a place and a path,
/// the caller to the Rules Engine's `PlaceReader`, and `RulesEngine::may_read` answers. A `place`
/// source names its place (`place:<place_id>:<path>`); a `sharepoint` or `confluence` source is the
/// working-copy item of that origin path in the workspace's active link of that kind. A source that
/// resolves to nothing is not readable; a module that cannot answer is `RulesUnavailable`, which
/// withholds the page (fail closed).
pub struct DesktopKnowledgeRules {
    pub rules: Arc<RulesEngine>,
    pub files: Arc<FileService>,
    pub workspaces: Arc<WorkspaceService>,
    pub late: Arc<Late>,
}

impl DesktopKnowledgeRules {
    async fn reader(&self, check: &SourceReadCheck) -> Result<PlaceReader, RulesUnavailable> {
        Ok(match &check.caller {
            ToolCaller::Ordin => PlaceReader::Ordin,
            ToolCaller::Agent => PlaceReader::Agent {
                agent_instance_id: check.agent_instance_id,
            },
            ToolCaller::CrewMember { crew_run_id } => {
                let orchestrator = self.late.orchestrator.get().ok_or(RulesUnavailable)?;
                let (crew, _) = orchestrator
                    .get(*crew_run_id)
                    .await
                    .map_err(|_| RulesUnavailable)?;
                PlaceReader::CrewMember {
                    crew_charter_id: crew.crew_charter_id,
                    agent_instance_id: check.agent_instance_id,
                }
            }
            ToolCaller::HostedAgent => PlaceReader::HostedAgent,
        })
    }

    /// The place and path of a source; `None` when it names nothing on this computer.
    async fn locate(
        &self,
        check: &SourceReadCheck,
    ) -> Result<Option<(PlaceId, String)>, RulesUnavailable> {
        let kind = match check.origin.as_str() {
            "place" => return self.locate_place(&check.location).await,
            "sharepoint" => SourceLinkKind::SharepointLocation,
            "confluence" => SourceLinkKind::ConfluenceLocation,
            _ => return Ok(None),
        };
        let links = self
            .workspaces
            .source_links(check.workspace_id)
            .await
            .map_err(|_| RulesUnavailable)?;
        for link in links.into_iter().filter(|l| l.kind == kind) {
            let Some(place_id) = link.place_id else {
                continue;
            };
            let items = self
                .workspaces
                .working_copy_items(link.id)
                .await
                .map_err(|_| RulesUnavailable)?;
            if items.iter().any(|i| i.origin_path == check.location) {
                return Ok(Some((place_id, check.location.clone())));
            }
        }
        Ok(None)
    }

    /// `place:<place_id>:<relative path>`; a place that is not one of this installation is none.
    async fn locate_place(
        &self,
        location: &str,
    ) -> Result<Option<(PlaceId, String)>, RulesUnavailable> {
        let Some((id, path)) = location
            .strip_prefix("place:")
            .and_then(|rest| rest.split_once(':'))
        else {
            return Ok(None);
        };
        let Ok(place_id) = PlaceId::parse(id) else {
            return Ok(None);
        };
        match self.files.place(place_id).await {
            Ok(_) => Ok(Some((place_id, path.to_owned()))),
            Err(FileError::PlaceNotFound) => Ok(None),
            Err(_) => Err(RulesUnavailable),
        }
    }
}

#[async_trait::async_trait]
impl KnowledgeRules for DesktopKnowledgeRules {
    // spec: ordin-desktop/components/KnowledgeService#SA-RECON-09
    async fn may_read_source(&self, check: &SourceReadCheck) -> Result<bool, RulesUnavailable> {
        let Some((place_id, path)) = self.locate(check).await? else {
            return Ok(false);
        };
        let reader = self.reader(check).await?;
        Ok(self.rules.may_read(&PlaceRead {
            workspace_id: check.workspace_id,
            reader,
            place_id,
            path,
        }))
    }
}

// ---------------------------------------------------------------- KnowledgeMemory

/// `KnowledgeMemory` over the Memory Service: every call is forwarded one for one, with the
/// workspace and read folder the Knowledge Service passes (DSK-REC-12).
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-12
pub struct DesktopKnowledgeMemory {
    pub memory: Arc<MemoryService>,
}

impl DesktopKnowledgeMemory {
    pub fn new(memory: Arc<MemoryService>) -> DesktopKnowledgeMemory {
        DesktopKnowledgeMemory { memory }
    }
}

#[async_trait::async_trait]
impl KnowledgeMemory for DesktopKnowledgeMemory {
    async fn index_page_sections(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
        sections: Vec<PageSection>,
        version: String,
        graph: PageGraph,
    ) -> Result<(), MemoryError> {
        self.memory
            .index_page_sections(workspace_id, page_id, sections, version, graph)
            .await
    }

    async fn remove_page(
        &self,
        workspace_id: WorkspaceId,
        folder_id: Option<TeamFolderId>,
        page_id: KnowledgePageId,
    ) -> Result<(), MemoryError> {
        self.memory
            .remove_page(workspace_id, folder_id, page_id)
            .await
    }

    async fn source_changed(
        &self,
        origin_ref: &str,
        current_version: &str,
    ) -> Result<u32, MemoryError> {
        self.memory
            .source_changed(origin_ref, current_version)
            .await
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
        self.memory
            .index_read_page_sections(workspace_id, folder_id, page_id, sections, version, graph)
            .await
    }

    async fn check_content(
        &self,
        workspace_id: WorkspaceId,
        text: &str,
    ) -> Result<ContentCheck, MemoryError> {
        self.memory
            .check_content(workspace_id, text.to_owned())
            .await
    }
}

// ---------------------------------------------------------------- KnowledgePush

/// `KnowledgePush` over the Push Service.
pub struct DesktopKnowledgePush {
    pub push: Arc<PushService>,
}

#[async_trait::async_trait]
impl KnowledgePush for DesktopKnowledgePush {
    async fn publish_copy(&self, req: PublishCopyRequest) -> Result<PublishedCopyId, PushError> {
        self.push.publish_copy(req).await
    }
}

// ---------------------------------------------------------------- KnowledgeFolders

/// `KnowledgeFolders` with no Team Knowledge Service: the `team-knowledge` switch is off in this
/// build (DSK-UP-5), so no folder has a label and no page was adopted from one, as for an absent
/// `MemoryFolders`.
pub struct NoKnowledgeFolders;

#[async_trait::async_trait]
impl KnowledgeFolders for NoKnowledgeFolders {
    async fn label_for(
        &self,
        _workspace_id: WorkspaceId,
        _folder_id: TeamFolderId,
        _item_path: &str,
    ) -> Option<FolderLabel> {
        None
    }

    async fn adoption_for(
        &self,
        _workspace_id: WorkspaceId,
        _page_id: KnowledgePageId,
    ) -> Option<AdoptionLabel> {
        None
    }
}

// ---------------------------------------------------------------- KnowledgeIdentity

/// `KnowledgeIdentity` over Identity and Credentials: the person's GitHub login
/// (`PersonProfile.github_login`, SA-OKF-03); none when it cannot be read.
pub struct DesktopKnowledgeIdentity {
    pub identity: Arc<IdentityService>,
}

#[async_trait::async_trait]
impl KnowledgeIdentity for DesktopKnowledgeIdentity {
    async fn git_login(&self) -> Option<String> {
        self.identity.person().await.ok()?.github_login
    }
}
