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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// The fact's triple (rule FV′; ADR-118).
    // spec: ordin-desktop/components/MemoryService#MEM-TR-7
    pub triple: Option<TripleView>,
    /// A fact written before facts were triples (MEM-TR-7).
    pub older_note: bool,
    /// Recall results only: a passage (MEM-TR-6).
    pub passage: bool,
    /// ≤ 8 live facts recorded in conflict with this one that the reader may read.
    pub conflicts_with: Vec<MemoryFactId>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    triple: Option<TripleView>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    older_note: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    passage: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    conflicts_with: Vec<MemoryFactId>,
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
            triple: v.triple,
            older_note: v.older_note,
            passage: v.passage,
            conflicts_with: v.conflicts_with,
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
    /// The term's label when `kind` is a term key of the place (ADR-118).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
    /// Format 2 (WIRE-TR-6): the fact's triple, its entities as ids; `None` for an older note.
    // spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TR-6
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triple: Option<TeamTriple>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub single_valued: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub changed: bool,
}

/// A team fact's triple (TeamKnowledgeService.md amendment ADR-118).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamTriple {
    /// "entity:<uuid>", one of the fact's `entities`.
    pub subject: String,
    /// A term key.
    pub predicate: String,
    /// The memory component's `TripleObject`; an entity object names "entity:<uuid>".
    pub object: serde_json::Value,
}

/// One link word of a team folder (`terms/<key>.json`, WIRE-TR-6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamTerm {
    /// 2.
    pub format: u8,
    pub key: String,
    /// 1–64.
    pub label: String,
    /// 1–300.
    pub meaning: String,
    /// "thing" | "text" | "number" | "money" | "date" | "yes_no".
    pub object: String,
    pub single_valued: bool,
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
    /// The place's live terms named by an exported fact (rule E, ADR-118).
    #[serde(default)]
    pub terms: Vec<TeamTerm>,
    pub entities: Vec<TeamEntity>,
    pub sources: Vec<TeamSource>,
    pub relationships: Vec<TeamRelationship>,
    pub held_for_check: u32,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamImportBatch {
    pub facts: Vec<TeamFact>,
    /// The terms the batch's facts name (rule F item 2, ADR-118).
    #[serde(default)]
    pub terms: Vec<TeamTerm>,
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
    /// Ordin's summaries naming the thing, newest first, ≤ 20 (ADR-118).
    pub summaries: Vec<FactView>,
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

// --- Facts as triples, the vocabulary, conflicts, passages and the diagram (ADR-118, ADR-120) ---

/// One fact as a caller states it (ADR-118 item 9).
// spec: ordin-desktop/components/MemoryService#MEM-TR-9
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TripleInput {
    /// Name 1–200, other names 0–8, and an optional topic (MEM-TOP-3).
    pub subject: SubjectInput,
    pub predicate: PredicateInput,
    pub object: ObjectInput,
    #[serde(default)]
    pub changed: bool,
    #[serde(default)]
    pub ended: bool,
    /// 0–1000 (MEM-TR-9).
    #[serde(default)]
    pub confidence_permille: Option<u16>,
    /// `YYYY` | `YYYY-MM` | `YYYY-MM-DD`, not later than today (MEM-TR-9).
    #[serde(default)]
    pub valid_from: Option<String>,
}

/// A fact's subject: a thing and, for a thing that has none yet, an optional topic (MEM-TOP-3,
/// amended January 2026 (user decision; Approved by the user January 2026)).
// spec: ordin-desktop/components/MemoryService#MEM-TOP-3
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SubjectInput {
    /// 1–200 characters.
    pub name: String,
    /// 0–8, each 1–200 characters.
    #[serde(default)]
    pub other_names: Vec<String>,
    /// 1–40 characters, a short plain label such as "Funding" (MEM-TOP-2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}

impl SubjectInput {
    /// The subject as a thing, without its topic.
    pub fn thing(&self) -> ThingInput {
        ThingInput {
            name: self.name.clone(),
            other_names: self.other_names.clone(),
        }
    }
}

impl From<ThingInput> for SubjectInput {
    fn from(thing: ThingInput) -> Self {
        SubjectInput {
            name: thing.name,
            other_names: thing.other_names,
            topic: None,
        }
    }
}

/// Who set a thing's topic (MEM-TOP-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicSetBy {
    Ordin,
    Agent,
    HostedAgent,
    Person,
}

impl TopicSetBy {
    /// The stored form (`memory_thing_topics.set_by`).
    pub fn as_str(self) -> &'static str {
        match self {
            TopicSetBy::Ordin => "ordin",
            TopicSetBy::Agent => "agent",
            TopicSetBy::HostedAgent => "hosted_agent",
            TopicSetBy::Person => "person",
        }
    }

    /// The kind a stored `set_by` names.
    pub fn parse(raw: &str) -> Option<TopicSetBy> {
        Some(match raw {
            "ordin" => TopicSetBy::Ordin,
            "agent" => TopicSetBy::Agent,
            "hosted_agent" => TopicSetBy::HostedAgent,
            "person" => TopicSetBy::Person,
            _ => return None,
        })
    }
}

/// One thing's topic (MEM-TOP-4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThingTopic {
    pub thing_id: MemoryThingId,
    pub topic: String,
    pub set_by: TopicSetBy,
    pub set_at: Timestamp,
}

/// The place's topics (MEM-TOP-4): every thing's topic, at most 5,000, by thing id; and the distinct
/// topics, by name, at most 200.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThingTopicsView {
    pub things: Vec<ThingTopic>,
    pub topics: Vec<String>,
}

/// A term key, or a proposed new word.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PredicateInput {
    /// A term key.
    Term(String),
    Propose {
        propose: TermProposal,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TermProposal {
    /// 1–64.
    pub label: String,
    /// 1–300.
    pub meaning: String,
    pub object: ObjectKindInput,
    pub single_valued: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKindInput {
    Thing,
    Text,
    Number,
    Money,
    Date,
    YesNo,
}

impl ObjectKindInput {
    /// The stored form (`memory_terms.object_kind`).
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectKindInput::Thing => "thing",
            ObjectKindInput::Text => "text",
            ObjectKindInput::Number => "number",
            ObjectKindInput::Money => "money",
            ObjectKindInput::Date => "date",
            ObjectKindInput::YesNo => "yes_no",
        }
    }

    /// The kind a stored `object_kind` names.
    pub fn parse(raw: &str) -> Option<ObjectKindInput> {
        Some(match raw {
            "thing" => ObjectKindInput::Thing,
            "text" => ObjectKindInput::Text,
            "number" => ObjectKindInput::Number,
            "money" => ObjectKindInput::Money,
            "date" => ObjectKindInput::Date,
            "yes_no" => ObjectKindInput::YesNo,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ObjectInput {
    Thing(ThingInput),
    /// 1–1,000; in `remember_facts`, one claim of 1–120 (MEM-TR-13).
    Text(String),
    Number {
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    Money {
        amount: String,
        currency: String,
    },
    /// `YYYY` | `YYYY-MM` | `YYYY-MM-DD`.
    Date(String),
    YesNo(bool),
}

impl ObjectInput {
    /// The object's kind.
    pub fn kind(&self) -> ObjectKindInput {
        match self {
            ObjectInput::Thing(_) => ObjectKindInput::Thing,
            ObjectInput::Text(_) => ObjectKindInput::Text,
            ObjectInput::Number { .. } => ObjectKindInput::Number,
            ObjectInput::Money { .. } => ObjectKindInput::Money,
            ObjectInput::Date(_) => ObjectKindInput::Date,
            ObjectInput::YesNo(_) => ObjectKindInput::YesNo,
        }
    }
}

/// 1–16 facts sharing one source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RememberFactsInput {
    #[serde(default)]
    pub source: Option<FactSourceInput>,
    /// 1–128; an agent's memory class.
    #[serde(default)]
    pub fact_class: Option<String>,
    /// 1–16.
    pub facts: Vec<TripleInput>,
}

/// Serde shape (internal; never a tool's or command's JSON): `"conversation"` |
/// `{"document": {"origin_ref", "version"}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactSourceInput {
    Conversation,
    Document(FactSource),
}

/// In input order.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RememberFactsReceipt {
    pub items: Vec<RememberReceipt>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TripleView {
    pub subject: ThingRef,
    pub predicate: TermRef,
    pub object: ObjectView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TermRef {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectView {
    Thing {
        thing: ThingRef,
    },
    Text {
        value: String,
    },
    Number {
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    Money {
        amount: String,
        currency: String,
    },
    Date {
        value: String,
    },
    YesNo {
        value: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TermView {
    pub key: String,
    pub label: String,
    pub meaning: String,
    pub object: ObjectKindInput,
    pub single_valued: bool,
    pub added_by: TermOrigin,
    pub fact_count: u32,
    pub seed_unrenamed: bool,
}

/// The hosted agent has no agent instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TermOrigin {
    Seed,
    Ordin,
    Agent { agent_instance_id: AgentInstanceId },
    HostedAgent,
    Team,
    Person,
}

/// ≤ 500, by label.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct VocabularyView {
    pub terms: Vec<TermView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MergeTermReceipt {
    pub kept: TermView,
    pub rewritten: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PassageView {
    pub id: MemoryFactId,
    pub heading: Option<String>,
    pub text: String,
    pub version: String,
    pub written_at: Timestamp,
}

/// ≤ 200, in section order.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PassagePage {
    pub items: Vec<PassageView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DiagramView {
    pub centre: Option<ThingRef>,
    /// ≤ 40.
    pub things: Vec<DiagramThing>,
    /// ≤ 200: live facts whose object is a drawn thing.
    pub joins: Vec<DiagramJoin>,
    /// ≤ 40: the centre's value facts; empty without a centre.
    pub values: Vec<DiagramValue>,
    /// Pairs among joins and values, smaller id first.
    pub conflicts: Vec<(MemoryFactId, MemoryFactId)>,
    /// Things the view could draw (within two steps, or in the workspace).
    pub total_things: u32,
    /// `total_things − things.len()`.
    pub left_out: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DiagramThing {
    pub thing: ThingRef,
    /// 0–2 with a centre.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<u8>,
    pub fact_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DiagramJoin {
    pub fact_id: MemoryFactId,
    pub from: MemoryThingId,
    pub to: MemoryThingId,
    pub word: TermRef,
    pub conflict: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DiagramValue {
    pub fact_id: MemoryFactId,
    pub word: TermRef,
    pub object: ObjectView,
    pub conflict: bool,
}

/// Whether Ordin keeps what it learns from a conversation on its own or holds it for the person
/// (Settings › Memory; MEM-SET-2; amended January 2026; user decision; Approved by the user January
/// 2026).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeepMode {
    /// Today's behaviour: the write gate alone decides.
    OnItsOwn,
    /// Every fact Ordin takes from a conversation waits in **To check** for the person.
    AskFirst,
}

/// The behaviour settings of Settings › Memory, held by this service and enforced on every write
/// and recall (MEM-SET-1; amended January 2026; user decision; Approved by the user January 2026).
/// The defaults are today's behaviour.
// spec: ordin-desktop/components/MemoryService#MEM-SET-1
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySettings {
    pub keep: KeepMode,
    /// Ordin, crew members and agents may keep facts from conversations.
    pub learn_from_conversations: bool,
    /// Ordin, crew members and agents may keep facts from documents they read.
    pub learn_from_documents: bool,
    /// `None`: a fact is suggested for removal when its use has faded (the decay rule). `Some(days)`:
    /// when it has not been used for that many days, 1–3,650. A suggestion only; nothing is removed.
    pub suggest_removal_after_days: Option<u16>,
    /// A fact the person made "Remember everywhere" is used in every workspace.
    pub use_everywhere: bool,
    /// Ordin's answers name the remembered facts they relied on (read by Ordin Host).
    pub say_which_facts: bool,
}

impl Default for MemorySettings {
    fn default() -> MemorySettings {
        MemorySettings {
            keep: KeepMode::OnItsOwn,
            learn_from_conversations: true,
            learn_from_documents: true,
            suggest_removal_after_days: None,
            use_everywhere: true,
            say_which_facts: false,
        }
    }
}
