//! The public types of the crate (*Public Interface* and every amendment's additions).

use ordin_types::time::Timestamp;
pub use ordin_types::wire::knowledge_query::{
    KnowledgeHitKind, PageStatus, PageTier, SharedIdentity, UnansweredSource,
};
use ordin_types::{
    AgentInstanceId, CrewRunId, KnowledgeAnswerId, KnowledgePageId, MemoryFactId, MemoryLinkId,
    MemoryRecallId, MemoryThingId, RunId, ScopeId, TeamFolderId, WorkspaceId,
};
use serde::{Deserialize, Serialize};

/// Who asks. Never built from tool arguments.
#[derive(Debug, Clone, Copy)]
pub enum MemoryCaller {
    Ordin {
        workspace_id: WorkspaceId,
        run_id: RunId,
    },
    CrewMember {
        workspace_id: WorkspaceId,
        run_id: RunId,
        crew_run_id: CrewRunId,
        agent_instance_id: AgentInstanceId,
    },
    Person {
        workspace_id: WorkspaceId,
    },
    KnowledgeService {
        workspace_id: WorkspaceId,
    },
    HostedAgent {
        scope_id: ScopeId,
        run_id: RunId,
    },
    /// Agent task sessions (ADR-111 item 1; decision 91).
    Agent {
        workspace_id: WorkspaceId,
        run_id: RunId,
        agent_instance_id: AgentInstanceId,
    },
    /// Agent profile: a knowledge agent's reader and query server (SA-MKA-08).
    KnowledgeAgent {
        scope_id: ScopeId,
    },
}

/// Where a person's memory command acts: a workspace (desktop, personal server) or a hosted agent's
/// scope (agent profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPlace {
    Workspace(WorkspaceId),
    Scope(ScopeId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RememberInput {
    /// One plain sentence, 1–500 characters.
    pub statement: String,
    /// What the fact is about (becomes the memory component's `subject`), 1–200 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// The agent's memory class (ADR-090 `retains`), 1–128 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fact_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<FactSource>,
    /// 0–16.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<ThingInput>,
    /// 0–16.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<LinkInput>,
    /// Ordin only (`MemoryCaller::Ordin`); 2–64 kept facts of the same workspace; makes the fact a
    /// summary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub made_from: Vec<MemoryFactId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FactSource {
    /// Where the fact came from: a document's location, 1–2048 characters.
    pub origin_ref: String,
    /// The version read, 1–256 characters.
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ThingInput {
    /// 1–200 characters.
    pub name: String,
    /// 0–8, each 1–200 characters.
    #[serde(default)]
    pub other_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LinkInput {
    /// 1–200 characters.
    pub from: String,
    /// 1–64 characters.
    pub kind: String,
    /// 1–200 characters.
    pub to: String,
    #[serde(default)]
    pub ended: bool,
    /// 0–1000, default 1000.
    #[serde(default)]
    pub confidence_permille: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RememberReceipt {
    pub id: MemoryFactId,
    pub state: FactState,
}

fn default_cap() -> u8 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RecallInput {
    /// 1–4,096 characters. Exactly one of `query` and `searches`.
    #[serde(default)]
    pub query: Option<String>,
    /// 1–4 typed sub-queries; the first counts double.
    #[serde(default)]
    pub searches: Option<Vec<RecallSearch>>,
    /// 1–21 folder names as recall returns them, or "own"; restricts the recall before the search.
    #[serde(default)]
    pub folders: Option<Vec<String>>,
    /// 1–20, default 10; 1–50 for `MemoryCaller::KnowledgeAgent` (SA-MPD-01).
    #[serde(default = "default_cap")]
    pub result_cap: u8,
    #[serde(default)]
    pub wider: WiderMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RecallSearch {
    #[serde(rename = "type")]
    pub kind: RecallSearchKind,
    /// 1–500 characters.
    pub query: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecallSearchKind {
    Lex,
    Vec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WiderMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RecallResult {
    pub facts: Vec<FactView>,
    pub abstained: bool,
    pub recall_id: MemoryRecallId,
    /// ≤ 20: the links recall followed to reach a returned fact.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<LinkView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub wider: Vec<WiderFact>,
    /// The merged order (SA-MKA-05); empty when nothing was asked (then `facts` is the order).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<RecallSlot>,
    pub wider_status: WiderStatus,
}

/// `{"fact":0}` | `{"wider":0}`: an index into `facts` or `wider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallSlot {
    Fact(u8),
    Wider(u8),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WiderFact {
    pub statement: String,
    pub agent: String,
    pub repository: String,
    pub path: String,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_title: Option<String>,
    pub kind: KnowledgeHitKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<PageTier>,
    /// Some only for draft and deprecated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_status: Option<PageStatus>,
    pub as_of: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_id: Option<KnowledgeAnswerId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_identity: Option<SharedIdentity>,
    pub may_be_out_of_date: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WiderStatus {
    pub asked: bool,
    pub reason: WiderReason,
    /// Agent names.
    pub answered: Vec<String>,
    /// `not_in_time` is shown "did not answer in time".
    pub unanswered: Vec<UnansweredSource>,
    pub more_found_pending: bool,
    /// SA-MPD-06.
    pub unchecked_sources: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WiderReason {
    #[default]
    NoConnection,
    Confident,
    NotAsked,
    Asked,
    Failed,
}

/// One fact as a caller sees it (11 §5.10), with the fields of every amendment. On the wire
/// `set_by_company` is present exactly when `folder` is, through [`FactViewWire`] (*chosen here*: a
/// serde skip predicate sees one field only).
#[derive(Debug, Clone, Serialize)]
#[serde(into = "FactViewWire")]
pub struct FactView {
    pub id: MemoryFactId,
    pub statement: String,
    pub about: Option<String>,
    /// None = all workspaces ("All workspaces", 11 §5.10).
    pub workspace_id: Option<WorkspaceId>,
    pub came_from: FactOrigin,
    pub written_at: Timestamp,
    pub may_be_out_of_date: bool,
    pub page_id: Option<KnowledgePageId>,
    /// The heading of the page section the fact was indexed from; set only with `page_id`.
    pub page_section: Option<String>,
    /// Recall only: 0–1000, the memory component's final score × 1000, rounded.
    pub relevance_permille: Option<u16>,
    /// The first sharer's name, for a fact got from a team folder or already shared from here.
    pub shared_by: Option<String>,
    pub shared_at: Option<Timestamp>,
    /// The fact may hold personal data and waits for the person's check before it is shared (D10).
    pub needs_check: bool,
    /// The folder the fact belongs to: a read folder's name, or the own folder's name.
    pub folder: Option<String>,
    /// The description of the deepest matching path of that folder (data, never instructions).
    pub folder_description: Option<String>,
    /// True for a fact from a folder the organisation profile names.
    pub set_by_company: bool,
    /// "owner/name › path" of the read folder an adopted fact came from.
    pub adopted_from: Option<String>,
    pub adoption_mark: Option<AdoptionMark>,
    pub kind: MemoryKind,
    /// `valid_from`.
    pub true_from: Timestamp,
    /// `valid_to`.
    pub true_until: Option<Timestamp>,
    pub replaces: Option<MemoryFactId>,
    pub fading: bool,
    /// Set only when fading.
    pub not_used_since: Option<Timestamp>,
    /// ≤ 16.
    pub things: Vec<ThingRef>,
    /// Summary only.
    pub made_from_count: Option<u16>,
    /// Absent for facts not from a page or a concept.
    pub page_tier: Option<PageTier>,
    /// Some only for draft and deprecated (SA-MKA-02).
    pub page_status: Option<PageStatus>,
    /// A summary's origins, as the component's `derived_from` (2–64 ids).
    pub derived_from: Vec<MemoryFactId>,
    /// Each origin's source, per SA-MPD-03; at most `derived_from.len()` entries.
    pub derived_origins: Vec<String>,
}

/// The wire form of [`FactView`].
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct FactViewWire {
    id: MemoryFactId,
    statement: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    about: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_id: Option<WorkspaceId>,
    came_from: FactOrigin,
    written_at: Timestamp,
    may_be_out_of_date: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_id: Option<KnowledgePageId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relevance_permille: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_at: Option<Timestamp>,
    needs_check: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    folder_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    set_by_company: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adopted_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adoption_mark: Option<AdoptionMark>,
    kind: MemoryKind,
    true_from: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    true_until: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    replaces: Option<MemoryFactId>,
    fading: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    not_used_since: Option<Timestamp>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    things: Vec<ThingRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    made_from_count: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_tier: Option<PageTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_status: Option<PageStatus>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    derived_from: Vec<MemoryFactId>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    derived_origins: Vec<String>,
}

impl From<FactView> for FactViewWire {
    fn from(v: FactView) -> FactViewWire {
        FactViewWire {
            set_by_company: v.folder.as_ref().map(|_| v.set_by_company),
            id: v.id,
            statement: v.statement,
            about: v.about,
            workspace_id: v.workspace_id,
            came_from: v.came_from,
            written_at: v.written_at,
            may_be_out_of_date: v.may_be_out_of_date,
            page_id: v.page_id,
            page_section: v.page_section,
            relevance_permille: v.relevance_permille,
            shared_by: v.shared_by,
            shared_at: v.shared_at,
            needs_check: v.needs_check,
            folder: v.folder,
            folder_description: v.folder_description,
            adopted_from: v.adopted_from,
            adoption_mark: v.adoption_mark,
            kind: v.kind,
            true_from: v.true_from,
            true_until: v.true_until,
            replaces: v.replaces,
            fading: v.fading,
            not_used_since: v.not_used_since,
            things: v.things,
            made_from_count: v.made_from_count,
            page_tier: v.page_tier,
            page_status: v.page_status,
            derived_from: v.derived_from,
            derived_origins: v.derived_origins,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FactOrigin {
    Conversation {
        run_id: RunId,
    },
    Agent {
        agent_instance_id: AgentInstanceId,
        run_id: RunId,
    },
    Document {
        origin_ref: String,
        version: String,
    },
    /// An agent wrote this and named a document. The document was not read to check it.
    AgentCiting {
        agent_instance_id: AgentInstanceId,
        run_id: Option<RunId>,
        origin_ref: String,
        version: String,
    },
    KnowledgePage {
        page_id: KnowledgePageId,
    },
    /// A colleague's team file says so; `shared_by` names them.
    Team,
    Person,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactState {
    Pending,
    Kept,
    NotKept,
    Retired,
    Forgotten,
    Replaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Happened,
    Known,
    Summary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThingRef {
    pub id: MemoryThingId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LinkView {
    pub id: MemoryLinkId,
    pub from: ThingRef,
    pub kind: String,
    pub to: ThingRef,
    pub since: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<Timestamp>,
    pub by: LinkBy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkBy {
    Agent { agent_instance_id: AgentInstanceId },
    Ordin,
    Person,
    Page { page_id: KnowledgePageId },
    Team,
}

/// SA-PAGE-01 shape.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FactPage {
    pub items: Vec<FactView>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ForgetReceipt {
    pub id: MemoryFactId,
    pub forgotten_at: Timestamp,
    pub proof_digest: String,
    /// Always serialised; 0 when none.
    pub summaries_removed: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScopeErasureReceipt {
    pub scope_id: ScopeId,
    pub erased_at: Timestamp,
    pub rows_removed: u64,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PageSection {
    pub heading: String,
    pub text: String,
}

/// Owned here; the Knowledge Service builds it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PageGraph {
    /// 1–200.
    pub title: String,
    /// 0–8, each 1–200 (ADR-104 item 5).
    pub other_names: Vec<String>,
    /// 0–15: linked page and its title (title + 15 = the component's 16 entities).
    pub links_to: Vec<(KnowledgePageId, String)>,
    #[serde(default)]
    pub standing: PageStanding,
}

/// Owned here; copied into the Knowledge Service (`PageGraph` field).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PageStanding {
    pub tier: PageTier,
    pub status: PageStatus,
    pub past_stale_after: bool,
}

/// ADR-090 item 3, from the agent's `memory` part (`retains`, `excludes`, `retention.maxAgeDays`,
/// `store.location`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentMemoryRules {
    /// Classes the agent may keep; empty = the part names none, so every class may be kept.
    pub retains: Vec<String>,
    pub excludes: Vec<String>,
    /// 1–36,500; None = no age limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age_days: Option<u32>,
    /// Recorded only; the Memory Service always keeps facts in its own store (ADR-090 item 3).
    pub store_location: String,
}

/// The team types (TeamKnowledgeService.md *Shared Context* §2–§3). `ordin-team-knowledge` depends
/// on this crate, so the definition lives here and that crate re-exports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamFact {
    /// 1.
    pub format: u8,
    pub id: MemoryFactId,
    /// "fact:<uuid>".
    pub memory_fact_id: String,
    /// 1–4,000 characters.
    pub statement: String,
    /// 1–200.
    #[serde(default)]
    pub about: Option<String>,
    /// "episodic" | "semantic" | "consolidated".
    pub memory_class: String,
    /// 1–128.
    #[serde(default)]
    pub fact_class: Option<String>,
    /// "conversation" | "agent" | "document" | "person".
    pub origin_kind: String,
    /// "source:<uuid>".
    #[serde(default)]
    pub source_id: Option<String>,
    /// "entity:<uuid>", 1–64.
    pub entities: Vec<String>,
    pub valid_from: Timestamp,
    #[serde(default)]
    pub valid_to: Option<Timestamp>,
    /// 0–1000.
    pub confidence_permille: u16,
    /// "fact:<uuid>".
    #[serde(default)]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub superseded_by: Option<String>,
    /// 0–64.
    #[serde(default)]
    pub derived_from: Vec<String>,
    pub written_at: Timestamp,
    /// 1–200 in a file; "" from `export_team` when never shared (rule E-3).
    pub shared_by: String,
    pub shared_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamEntity {
    pub format: u8,
    pub id: String,
    /// 1–512.
    pub canonical_name: String,
    /// 0–32.
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamSource {
    pub format: u8,
    pub id: String,
    /// 1–2,048.
    pub origin_ref: String,
    /// 1–256.
    pub modification_marker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamRelationship {
    pub format: u8,
    pub id: String,
    /// 1–128.
    pub kind: String,
    pub from: String,
    pub to: String,
    pub valid_from: Timestamp,
    pub valid_to: Option<Timestamp>,
    pub confidence_permille: u16,
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamExportPage {
    pub facts: Vec<TeamFact>,
    pub entities: Vec<TeamEntity>,
    pub sources: Vec<TeamSource>,
    pub relationships: Vec<TeamRelationship>,
    pub held_for_check: u32,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamImportBatch {
    pub facts: Vec<TeamFact>,
    pub entities: Vec<TeamEntity>,
    pub sources: Vec<TeamSource>,
    pub relationships: Vec<TeamRelationship>,
    /// None = the workspace's own folder (unchanged behaviour); Some = a read folder.
    pub read_folder: Option<TeamFolderId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeamImportOutcome {
    pub item_id: String,
    pub result: TeamImportResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TeamImportResult {
    Kept,
    Unchanged,
    NotTaken { code: &'static str },
}

pub struct FolderLabel {
    pub name: String,
    pub description: Option<String>,
    pub set_by_company: bool,
}

pub struct AdoptionLabel {
    pub adopted_from: String,
    pub mark: Option<AdoptionMark>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionMark {
    ChangedAtSource,
    RemovedAtSource,
}

/// A trait this crate owns, implemented by the program over the Team Knowledge Service (D3; no edge
/// from this crate to `ordin-team-knowledge`, which already depends on this one).
#[async_trait::async_trait]
pub trait MemoryFolders: Send + Sync {
    /// Folder names as recall returns them, or "own", to collections ("" for "own", "tk:<id>" for a
    /// read folder). An unknown name → Err(the name).
    async fn collections_for(
        &self,
        workspace_id: WorkspaceId,
        names: &[String],
    ) -> Result<Vec<String>, String>;
    /// None = the workspace's own folder. Returns None when the workspace has no such folder (a
    /// workspace not in Team Knowledge mode has no label for its own facts).
    async fn label_for(
        &self,
        workspace_id: WorkspaceId,
        read_folder_id: Option<TeamFolderId>,
        item_path: &str,
    ) -> Option<FolderLabel>;
    async fn adoption_for(&self, workspace_id: WorkspaceId, item_id: &str)
        -> Option<AdoptionLabel>;
}

/// A trait this crate owns, implemented by the program over the Knowledge Service (the same pattern
/// as [`MemoryFolders`]).
#[async_trait::async_trait]
pub trait MemoryPages: Send + Sync {
    /// The page whose title or one of whose other names equals one of `names` after
    /// `normalise_name`; the workspace's own pages first, then read-folder pages; the most recently
    /// updated first. None = no page.
    async fn page_explaining(
        &self,
        workspace_id: WorkspaceId,
        names: &[String],
    ) -> Option<PageLabel>;
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PageLabel {
    pub page_id: KnowledgePageId,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThingView {
    pub id: MemoryThingId,
    pub name: String,
    pub other_names: Vec<String>,
    pub fact_count: u32,
    pub link_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explained_in: Option<PageLabel>,
}

/// SA-PAGE-01 shape, 100 a page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThingPage {
    pub items: Vec<ThingView>,
    pub next_cursor: Option<String>,
}

/// ≤ 200 each.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThingDetail {
    pub thing: ThingView,
    pub facts: Vec<FactView>,
    pub links: Vec<LinkView>,
}

/// Oldest first.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FactHistoryView {
    pub versions: Vec<FactView>,
    pub links: Vec<LinkView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginsView {
    pub items: Vec<OriginItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginItem {
    pub fact: FactView,
    pub state: OriginState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginState {
    Current,
    NoLongerTrue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryUsedView {
    pub facts: Vec<FactView>,
    pub links: Vec<LinkView>,
    /// Items forgotten since.
    pub gone: u16,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub wider: Vec<WiderUsed>,
    /// Any recall of the run had unchecked sources.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub wider_unchecked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WiderUsed {
    pub agent: String,
    pub repository: String,
    pub path: String,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_title: Option<String>,
    pub as_of: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_id: Option<KnowledgeAnswerId>,
    pub more_found: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MoreFoundView {
    pub recall_id: MemoryRecallId,
    pub wider: Vec<WiderFact>,
    pub unanswered: Vec<UnansweredSource>,
    pub done: bool,
    pub expired: bool,
}

/// Oldest first, ≤ 500.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RememberedInRun {
    pub items: Vec<RememberedItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RememberedItem {
    pub fact: FactView,
    pub by: RememberedBy,
}

/// SA-AMEM-07: the agent instance identifier, no name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RememberedBy {
    Ordin,
    Agent { agent_instance_id: AgentInstanceId },
    CrewMember { agent_instance_id: AgentInstanceId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ThingCount {
    pub facts: u32,
    pub links: u32,
    pub summaries: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThingForgetReceipt {
    pub id: MemoryThingId,
    pub facts_removed: u32,
    pub links_removed: u32,
    pub summaries_removed: u32,
    pub forgotten_at: Timestamp,
    pub proof_digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LinkForgetReceipt {
    pub id: MemoryLinkId,
    pub forgotten_at: Timestamp,
    pub proof_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ForgetCount {
    pub summaries: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThingSummary {
    pub id: MemoryThingId,
    pub name: String,
    /// ≤ 32.
    pub other_names: Vec<String>,
    pub link_count: u32,
    pub fact_count: u32,
}

/// One version of one document of a knowledge agent's repositories (KnowledgeReader.md).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeDocumentFacts {
    /// "git:<repository key>/<path>"; 1–2,048 characters.
    pub origin_prefix: String,
    /// By scheme (SA-MPD-05).
    pub version: String,
    /// 0–1000; SA-MKA-01.
    pub trust_permille: u16,
    pub standing: DocumentStanding,
    /// 0–200; 0 retires the earlier version only.
    pub facts: Vec<KnowledgeFactInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeFactInput {
    /// SA-MKA-11.
    pub statement: String,
    /// 1–200.
    #[serde(default)]
    pub about: Option<String>,
    /// 0–16.
    #[serde(default)]
    pub entities: Vec<ThingInput>,
    /// 0–16.
    #[serde(default)]
    pub links: Vec<LinkInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DocumentStanding {
    pub kind: SourceKind,
    /// Pages and glossaries only.
    #[serde(default)]
    pub tier: Option<PageTier>,
    #[serde(default)]
    pub status: Option<PageStatus>,
    #[serde(default)]
    pub stale_after: Option<Timestamp>,
    /// Pages and glossaries only, 1–200.
    #[serde(default)]
    pub page_title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Page,
    Document,
    Glossary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeIndexReceipt {
    pub written: u16,
    pub retired: u32,
}

/// A thing a held write names, with every other name a keep gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldThing {
    pub name: String,
    pub other_names: Vec<String>,
}

/// A link a held write states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldLink {
    pub from: String,
    pub kind: String,
    pub to: String,
    pub ended: bool,
    pub confidence_permille: Option<u16>,
}

/// One write the gate holds for a person (SA-TRUST-04).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeldWrite {
    pub id: String,
    pub statement: String,
    pub about: Option<String>,
    pub fact_class: Option<String>,
    /// The document it cites, if any.
    pub cited: Option<String>,
    /// Everything a keep writes beside the statement.
    pub things: Vec<HeldThing>,
    pub links: Vec<HeldLink>,
    /// Handed back to `keep_held`, so that what is kept is what the person saw.
    pub content_hash: String,
    /// The gate's score, 0–1000.
    pub trust_permille: u16,
    /// The agent that wrote it; `None` for a team file's fact.
    pub written_by: Option<AgentInstanceId>,
    pub held_at: Timestamp,
    /// Personal data the scan could not settle, as `pii_review` on a kept fact.
    pub needs_check: bool,
}
