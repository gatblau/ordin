//! The traits this crate owns and the program implements over each owner (*Shared Context* § Calls
//! into other modules, D3), the traits it implements for its callers, and `KnowledgeDeps`.
//! Each method keeps its owner's signature, so the program's implementation is a delegation.

use std::sync::Arc;

use ordin_files::{
    BinTicket, Cursor, FileBytes, FileEntry, FileError, FileVersion, FolderSort, Page, PlaceChange,
    WriteVersion,
};
use ordin_journal::Journal;
use ordin_memory_service::{
    AdoptionLabel, ContentCheck, FolderLabel, MemoryError, PageGraph, PageSection,
};
use ordin_push::{PublishCopyRequest, PushError};
use ordin_types::domain::places::{SourceLinkKind, Workspace};
use ordin_types::session::ToolCaller;
use ordin_types::time::Timestamp;
use ordin_types::{
    AgentInstanceId, KnowledgePageId, PlaceId, PublishedCopyId, RunId, ScopeId, TeamFolderId,
    WorkspaceId,
};
use ordin_workspaces::{OriginChanged, OriginVersion, WorkspaceError};

use crate::error::KnowledgeError;
use crate::model::{
    ChatHistoryError, ChatHistoryPage, IngestItem, IngestStep, ItemKind, KnowledgeItem,
    KnowledgeSourceConfig, PageHit, PageSummary, ScopeChat, SearchResult, Sender,
};

// spec: ordin-desktop/components/KnowledgeService#SA-RECON-08
#[async_trait::async_trait]
pub trait KnowledgeFiles: Send + Sync {
    // FileService
    async fn write_version(&self, w: WriteVersion) -> Result<FileVersion, FileError>;
    async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError>;
    async fn current_version(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<Option<FileVersion>, FileError>;
    async fn move_to_bin(&self, place_id: PlaceId, path: &str) -> Result<BinTicket, FileError>;
    async fn restore_from_bin(&self, ticket: &BinTicket) -> Result<FileVersion, FileError>;
    fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<PlaceChange>;
    /// The agent's own `knowledge/` folder (SA-RECON-08): Markdown files, at most 1 MiB each.
    async fn list_agent_knowledge(
        &self,
        agent: AgentInstanceId,
    ) -> Result<Vec<AgentKnowledgeFile>, FileError>;
    async fn read_agent_knowledge(
        &self,
        agent: AgentInstanceId,
        relative_path: &str,
    ) -> Result<Vec<u8>, FileError>;
    // amended January 2026 (ADR-104): FileService.md — Moving a file
    async fn move_file(
        &self,
        place_id: PlaceId,
        from: &str,
        to: &str,
    ) -> Result<FileVersion, FileError>;
    async fn versions(&self, place_id: PlaceId, path: &str) -> Result<Vec<FileVersion>, FileError>;
    async fn list_folder(
        &self,
        place_id: PlaceId,
        folder: &str,
        sort: FolderSort,
        cursor: Option<Cursor>,
    ) -> Result<Page<FileEntry>, FileError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentKnowledgeFile {
    pub relative_path: String,
    pub bytes: u64,
}

#[async_trait::async_trait]
pub trait KnowledgeWorkspaces: Send + Sync {
    // WorkspaceService
    /// WS_NOT_FOUND when absent.
    async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;
    /// Every workspace that is not in the bin (`WorkspaceService::list` with
    /// `WorkspaceListFilter::NotInBin`), so a workspace with no pages yet is still found (FU-103).
    async fn list_not_in_bin(&self) -> Result<Vec<Workspace>, WorkspaceError>;
    async fn origin_version(
        &self,
        workspace_id: WorkspaceId,
        link_kind: SourceLinkKind,
        origin_path: &str,
    ) -> Result<Option<OriginVersion>, WorkspaceError>;
    fn subscribe_origin_changes(&self) -> tokio::sync::broadcast::Receiver<OriginChanged>;
    // PackageCatalogue
    async fn agents_in_workspace(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<WorkspaceAgent>, PackageError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAgent {
    pub agent_instance_id: AgentInstanceId,
    pub display_name: String,
}

/// The Package Catalogue's error, as `agents_in_workspace` returns it.
// This crate has no edge to `ordin-package-catalogue`, so the error is declared here as the code
// the program's implementation passes through (KnowledgeService.md, SA-KBUILT-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageError {
    pub code: &'static str,
}

#[async_trait::async_trait]
pub trait KnowledgeRules: Send + Sync {
    /// The program resolves a `sharepoint`/`confluence` source to its working-copy place, maps the
    /// caller to the Rules Engine's `PlaceReader` and asks `RulesEngine::may_read` (SA-RECON-09).
    async fn may_read_source(&self, check: &SourceReadCheck) -> Result<bool, RulesUnavailable>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReadCheck {
    pub workspace_id: WorkspaceId,
    pub caller: ToolCaller,
    pub agent_instance_id: AgentInstanceId,
    pub origin: String,
    pub location: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RulesUnavailable;

// The two methods `remove_page` and `check_content` take the Memory Service's own form, with the
// workspace and read folder this crate holds at each call; the program forwards them one for one.
// spec: ordin-desktop/components/KnowledgeService#KNOW-REC-1
#[async_trait::async_trait]
pub trait KnowledgeMemory: Send + Sync {
    // MemoryService
    async fn index_page_sections(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
        sections: Vec<PageSection>,
        version: String,
        graph: PageGraph,
    ) -> Result<(), MemoryError>;
    async fn remove_page(
        &self,
        workspace_id: WorkspaceId,
        folder_id: Option<TeamFolderId>,
        page_id: KnowledgePageId,
    ) -> Result<(), MemoryError>;
    async fn source_changed(
        &self,
        origin_ref: &str,
        current_version: &str,
    ) -> Result<u32, MemoryError>;
    // amended January 2026 (ADR-110, ADR-111)
    async fn index_read_page_sections(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
        page_id: KnowledgePageId,
        sections: Vec<PageSection>,
        version: String,
        graph: PageGraph,
    ) -> Result<(), MemoryError>;
    async fn check_content(
        &self,
        workspace_id: WorkspaceId,
        text: &str,
    ) -> Result<ContentCheck, MemoryError>;
}

#[async_trait::async_trait]
pub trait KnowledgePush: Send + Sync {
    // PushService
    async fn publish_copy(&self, req: PublishCopyRequest) -> Result<PublishedCopyId, PushError>;
}

/// Implemented by the program over the Team Knowledge Service (ADR-110; D3).
#[async_trait::async_trait]
pub trait KnowledgeFolders: Send + Sync {
    /// Name, deepest description of `item_path` (e.g. "pages/<id>.md") and company mark of a folder.
    async fn label_for(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
        item_path: &str,
    ) -> Option<FolderLabel>;
    /// For a page of the workspace that was adopted: the source folder's name and any mark.
    async fn adoption_for(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
    ) -> Option<AdoptionLabel>;
}

/// Implemented by the program over Identity and Credentials (ADR-112; D3).
#[async_trait::async_trait]
pub trait KnowledgeIdentity: Send + Sync {
    /// The person's Git host login, or None when no Git host sign-in is held (SA-OKF-03).
    async fn git_login(&self) -> Option<String>;
}

// ---------------------------------------------------------------- agent profile

#[async_trait::async_trait]
pub trait KnowledgeScopeSettings: Send + Sync {
    /// The stored value or the default; Unavailable maps to KNOW_DB_UNAVAILABLE in tools.
    async fn number(&self, scope: ScopeId, name: &str) -> Result<i64, SettingsReadError>;
    async fn flag(&self, scope: ScopeId, name: &str) -> Result<bool, SettingsReadError>;
    async fn selection(&self, scope: ScopeId, name: &str)
        -> Result<Vec<String>, SettingsReadError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsReadError {
    Unknown,
    Unavailable,
}

#[async_trait::async_trait]
pub trait KnowledgeRunContext: Send + Sync {
    /// The run's incoming message when it started with free text (wrightrun `run.input.message`).
    async fn question(&self, run_id: RunId) -> Option<String>;
    /// Records an item the run opened (wrightrun ledger `opened_items`), once per item.
    async fn item_opened(&self, run_id: RunId, item_id: uuid::Uuid);
}

/// wrightrun's counter names, unchanged (SA-OBS-02).
pub trait KnowledgeMetrics: Send + Sync {
    /// wrightrun_knowledge_queries_total
    fn query(&self, source: &str, operation: &'static str);
    /// wrightrun_notes_written_total
    fn note_written(&self, source: &str, operation: &'static str);
    /// wrightrun_note_writes_refused_total
    fn note_refused(&self, source: &str, reason: &'static str);
    /// wrightrun_knowledge_items_written_total (family 36, SA-CLOSE-02)
    fn items_written(&self, source: &str, count: u64);
    /// wrightrun_ingest_thread_failures_total (family 37, SA-CLOSE-02)
    fn ingest_thread_failed(&self, source: &str);
}

/// Chat history for the ingest (SA-RECON-01; `replies` added January 2026, ADR-107).
#[async_trait::async_trait]
pub trait ChatHistory: Send + Sync {
    /// The chat a scope stands for: `None` when the scope is unknown.
    async fn chat_of(&self, scope: ScopeId) -> Result<Option<ScopeChat>, ChatHistoryError>;
    /// One page of messages, oldest first, newer than `oldest` (exclusive).
    async fn page(
        &self,
        connection: &str,
        chat: &str,
        oldest: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<ChatHistoryPage, ChatHistoryError>;
    /// One page of the replies of one thread, oldest first, without the thread's first message.
    async fn replies(
        &self,
        connection: &str,
        chat: &str,
        thread: &str,
        cursor: Option<&str>,
    ) -> Result<ChatHistoryPage, ChatHistoryError>;
}

/// wrightrun's KnowledgeIndex protocol, in Rust.
#[async_trait::async_trait]
pub trait KnowledgeIndex: Send + Sync {
    async fn upsert(
        &self,
        source: &str,
        scope: ScopeId,
        items: &[IngestItem],
    ) -> Result<u64, KnowledgeError>;
    async fn search(
        &self,
        sources: &[String],
        scope: Option<ScopeId>,
        query: &str,
        question: Option<&str>,
        limit: u32,
    ) -> Result<Vec<SearchResult>, KnowledgeError>;
    async fn recent(
        &self,
        source: &str,
        scope: Option<ScopeId>,
        limit: u32,
    ) -> Result<Vec<KnowledgeItem>, KnowledgeError>;
    /// ADR-107: the search of one source for a question, run before the model; no model call.
    async fn lookup(
        &self,
        source: &str,
        scope: Option<ScopeId>,
        question: &str,
        limit: u32,
    ) -> Result<Vec<SearchResult>, KnowledgeError>;
    async fn open(
        &self,
        item_id: &str,
        scope: Option<ScopeId>,
    ) -> Result<Option<KnowledgeItem>, KnowledgeError>;
    async fn senders(&self, source: &str, scope: ScopeId) -> Result<Vec<Sender>, KnowledgeError>;
    async fn note_write(
        &self,
        source: &str,
        scope: Option<ScopeId>,
        text: &str,
        run_id: RunId,
    ) -> Result<uuid::Uuid, KnowledgeError>;
    async fn note_replace(
        &self,
        note_id: &str,
        scope: Option<ScopeId>,
        text: &str,
        run_id: RunId,
    ) -> Result<(), KnowledgeError>;
    async fn note_retire(
        &self,
        note_id: &str,
        scope: Option<ScopeId>,
    ) -> Result<(), KnowledgeError>;
    async fn notes(
        &self,
        scope: Option<ScopeId>,
        cursor: Option<&str>,
    ) -> Result<ordin_types::Page<KnowledgeItem>, KnowledgeError>;
    async fn entry_add(
        &self,
        source: &str,
        scope: ScopeId,
        text: &str,
        by: &str,
    ) -> Result<uuid::Uuid, KnowledgeError>;
    async fn entry_verify(&self, entry_id: &str, by: &str) -> Result<(), KnowledgeError>;
    async fn entry_remove(&self, entry_id: &str) -> Result<(), KnowledgeError>;
    async fn entries(
        &self,
        scope: ScopeId,
        cursor: Option<&str>,
    ) -> Result<ordin_types::Page<KnowledgeItem>, KnowledgeError>;
    async fn delete_older_than(
        &self,
        kind: ItemKind,
        cutoff: chrono::DateTime<chrono::Utc>,
        limit: u32,
    ) -> Result<u64, KnowledgeError>;
    async fn delete_by_sender(&self, connection: &str, sender: &str)
        -> Result<u64, KnowledgeError>;
    async fn delete_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError>;
    async fn count_by_sender(&self, connection: &str, sender: &str) -> Result<u64, KnowledgeError>;
    async fn count_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError>;
}

/// One page of an ingest (SA-RECON-01). Called by the Job Runner once per step.
#[async_trait::async_trait]
pub trait KnowledgeIngest: Send + Sync {
    /// Reads one page of the scope's chat history and indexes it with `upsert`.
    async fn ingest_step(
        &self,
        source: &str,
        scope: ScopeId,
        checkpoint: Option<&serde_json::Value>,
    ) -> Result<IngestStep, KnowledgeError>;
}

/// Home and Search's reads across workspaces (SA-RECON-03). Desktop and personal server only.
#[async_trait::async_trait]
pub trait KnowledgeReader: Send + Sync {
    async fn search_pages(
        &self,
        workspaces: &[WorkspaceId],
        query: &str,
        limit: u32,
    ) -> Result<Vec<PageHit>, KnowledgeError>;
    async fn out_of_date_since(&self, since: Timestamp)
        -> Result<Vec<PageSummary>, KnowledgeError>;
    async fn page_version(&self, id: KnowledgePageId) -> Result<Option<String>, KnowledgeError>;
}

// ---------------------------------------------------------------- construction

pub struct KnowledgeDeps {
    pub journal: Arc<Journal>,
    /// The File Service calls above, as a trait the program wires.
    pub files: Arc<dyn KnowledgeFiles>,
    pub workspaces: Arc<dyn KnowledgeWorkspaces>,
    pub rules: Arc<dyn KnowledgeRules>,
    pub memory: Arc<dyn KnowledgeMemory>,
    pub push: Arc<dyn KnowledgePush>,
    /// Agent profile only; None on the desktop and the personal server.
    pub agent: Option<AgentKnowledgeDeps>,
    /// ADR-110.
    pub folders: Arc<dyn KnowledgeFolders>,
    /// ADR-112.
    pub identity: Arc<dyn KnowledgeIdentity>,
    /// The program's `env!("CARGO_PKG_VERSION")`, used in `generated.by: ordin/<app_version>`.
    pub app_version: String,
}

pub struct AgentKnowledgeDeps {
    pub sources: Vec<KnowledgeSourceConfig>,
    pub settings: Arc<dyn KnowledgeScopeSettings>,
    pub metrics: Arc<dyn KnowledgeMetrics>,
    pub runs: Arc<dyn KnowledgeRunContext>,
    /// Chat history for the ingest step (SA-RECON-01).
    pub chat_history: Arc<dyn ChatHistory>,
    /// true when the configuration's `contract.capabilities` lists `notes` (wrightrun ADR-035).
    pub note_tools: bool,
}
