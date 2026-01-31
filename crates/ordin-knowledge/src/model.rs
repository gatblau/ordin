//! The data types of the Public Interface (KnowledgeService.md *Public Interface* and the
//! additions of its amendments), as written there.

use ordin_memory_service::AdoptionMark;
use ordin_types::domain::places::{
    KnowledgeAuthor, KnowledgePage, KnowledgePageState, KnowledgeSource,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::{PageStatus, PageTier};
use ordin_types::{
    AgentInstanceId, KnowledgePageId, KnowledgeSuggestionId, RunId, ScopeId, TeamFolderId,
    WorkspaceId,
};
use serde::{Deserialize, Serialize};

/// This crate's own value, passed to `new` by the program (*Shared Context*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramKind {
    Desktop,
    PersonalServer,
    AgentSingle,
    AgentScaled,
}

impl ProgramKind {
    pub(crate) fn is_agent(self) -> bool {
        matches!(self, ProgramKind::AgentSingle | ProgramKind::AgentScaled)
    }
}

// ---------------------------------------------------------------- workspace pages

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WritePage {
    pub workspace_id: WorkspaceId,
    /// None = a new page; Some = replace that page's body and sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_id: Option<KnowledgePageId>,
    pub title: String,
    pub body_markdown: String,
    pub sources: Vec<KnowledgeSource>,
    /// The run whose finish produced it (Ordin Host); journalled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    /// ADR-104: None = unchanged on a rewrite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    /// ADR-112: None = unchanged on a rewrite.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PageView {
    pub page: KnowledgePage,
    /// One entry per source with a newer version not yet left (empty when current).
    pub changes: Vec<SourceChange>,
    pub pending_update: bool,
    pub relative_path: String,
    // ADR-103
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_at: Option<Timestamp>,
    /// A `place` source names a place that is not on this computer (SA-KTEAM-03).
    pub cites_other_computer: bool,
    // ADR-104
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    /// Aliases kept in the file but not usable as names: another page holds that name.
    pub unregistered_aliases: Vec<String>,
    /// The title in the file when another page holds it; the page keeps its previous title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_refused: Option<String>,
    pub links_out: Vec<PageLinkView>,
    pub links_in: Vec<PageRef>,
    pub links_truncated: u32,
    pub suggestions: Vec<SuggestionView>,
    // ADR-110
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_by_company: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<TeamFolderId>,
    pub read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adopted_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adoption_mark: Option<AdoptionMark>,
    // ADR-112
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub trust: PageTier,
    /// Some only for draft and deprecated (stable is not shown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PageStatus>,
    pub past_stale_after: bool,
    /// Verifiers as written, newest first, at most 20 ("Checked by …").
    pub verified: Vec<VerifiedView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SourceChange {
    pub origin: String,
    pub location: String,
    pub version_read: String,
    pub newer_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PageListing {
    pub this_workspace: Vec<PageSummary>,
    pub from_agents: Vec<AgentReferencePage>,
    /// ADR-104.
    pub not_adopted: Vec<NotAdoptedFile>,
    /// ADR-110: read pages, ordered by folder name then title.
    pub from_other_teams: Vec<PageSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PageSummary {
    pub workspace_id: WorkspaceId,
    pub id: KnowledgePageId,
    pub title: String,
    pub state: KnowledgePageState,
    pub written_by: KnowledgeAuthor,
    pub last_changed_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_at: Option<Timestamp>,
    pub cites_other_computer: bool,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_by_company: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<TeamFolderId>,
    pub read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adopted_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adoption_mark: Option<AdoptionMark>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub trust: PageTier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PageStatus>,
    pub past_stale_after: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentReferencePage {
    pub agent_instance_id: AgentInstanceId,
    /// The agent's display name only, never its version (ADR-057, decision 56).
    pub agent_name: String,
    pub relative_path: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct UpdateDiff {
    pub page_id: KnowledgePageId,
    pub before: String,
    pub after: String,
    pub changed_lines: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PageHit {
    pub workspace_id: WorkspaceId,
    pub id: KnowledgePageId,
    pub title: String,
    pub state: KnowledgePageState,
    pub snippet: String,
    pub rank: f64,
}

/// Ordin's `show` block (2C wire §7), copied from *Shared Context*; the screens resolve it
/// through `resolve_block`.
// The wire type lives in `ordin-service-api`, to which this crate has no edge; this is the copy with
// the same fields and serde form, which the program converts from (KnowledgeService.md, SA-KBUILT-08).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeBlock {
    pub workspace_id: WorkspaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<KnowledgePageId>,
}

// ---------------------------------------------------------------- ADR-104

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PageRef {
    pub page_id: KnowledgePageId,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PageLinkView {
    /// None = no page of the workspace has that name ("No page called …").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<PageRef>,
    /// The target as written in the page, without `#section` and `|shown`.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    Link,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateReason {
    Names,
    Text,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SuggestionView {
    pub id: KnowledgeSuggestionId,
    pub kind: SuggestionKind,
    pub other: PageRef,
    /// Link: the words that would become the link, as they appear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_text: Option<String>,
    /// Link: up to 60 characters either side of the words, plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<DuplicateReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct NotAdoptedFile {
    pub relative_path: String,
    pub reason: NotAdoptedReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotAdoptedReason {
    PageLimit,
    TooLarge,
    HeaderInvalid,
    IdInOtherWorkspace,
}

// ---------------------------------------------------------------- ADR-112

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct VerifiedView {
    pub by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

// ---------------------------------------------------------------- ADR-111

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageNameMatch {
    pub page_id: KnowledgePageId,
    pub title: String,
}

// ---------------------------------------------------------------- ADR-103 / ADR-110 (team folders)

/// Owned by `ordin-team-knowledge` (TeamKnowledgeService.md *Shared Context* §2), copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamPageFile {
    pub page_id: KnowledgePageId,
    pub bytes: Vec<u8>,
    pub last_changed: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamPageImport {
    pub page_id: KnowledgePageId,
    pub bytes: Vec<u8>,
    pub both_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamPageImported {
    Written { version: u32 },
    KeptLocal { remote_version: u32 },
    Unchanged,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AdoptEdit {
    pub statement: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
}

// ---------------------------------------------------------------- agent profile (wrightrun)

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct KnowledgeSourceConfig {
    /// slug ^[a-z0-9][a-z0-9-]{0,56}$ (SA-RECON-04)
    pub name: String,
    pub kind: ItemKind,
    /// required with chat-history
    pub connection: Option<String>,
    /// ^[a-z_]{1,32}$
    #[serde(default = "english")]
    pub language: String,
    /// read by the Job Runner only
    pub ingest_cron: Option<String>,
    #[serde(default)]
    pub ranking: Ranking,
    /// 1..=50
    #[serde(default = "five")]
    pub default_results: u8,
    pub results_setting: Option<String>,
    pub own_messages_setting: Option<String>,
    pub hidden_senders_setting: Option<String>,
    #[serde(default)]
    pub expose: Vec<ExposedOperation>,
    /// required with notes; 1..=10000
    pub max_notes_per_scope: Option<u32>,
    /// required with notes; 1..=20000
    pub max_note_chars: Option<u32>,
}

fn english() -> String {
    "english".to_owned()
}

fn five() -> u8 {
    5
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemKind {
    ChatHistory,
    Entries,
    Notes,
}

impl ItemKind {
    pub(crate) fn text(self) -> &'static str {
        match self {
            ItemKind::ChatHistory => "chat-history",
            ItemKind::Entries => "entries",
            ItemKind::Notes => "notes",
        }
    }

    pub(crate) fn from_text(raw: &str) -> Option<ItemKind> {
        match raw {
            "chat-history" => Some(ItemKind::ChatHistory),
            "entries" => Some(ItemKind::Entries),
            "notes" => Some(ItemKind::Notes),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExposedOperation {
    Search,
    Recent,
    Open,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Ranking {
    #[serde(default)]
    pub thread_replies: bool,
    #[serde(default)]
    pub one_per_text: bool,
    #[serde(default)]
    pub favour_links_and_steps: Vec<QuestionType>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct QuestionType {
    pub name: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub r#match: MatchMode,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchMode {
    #[default]
    Any,
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngestItem {
    pub origin_id: String,
    pub origin_time: chrono::DateTime<chrono::Utc>,
    pub sender: String,
    pub sender_name: Option<String>,
    pub from_program: bool,
    pub own: bool,
    pub text: String,
    pub thread: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeItem {
    pub item_id: uuid::Uuid,
    pub source: String,
    pub kind: ItemKind,
    /// None = agent-wide
    pub scope_id: Option<ScopeId>,
    pub origin_id: Option<String>,
    pub origin_time: chrono::DateTime<chrono::Utc>,
    pub sender: Option<String>,
    pub sender_name: Option<String>,
    pub from_program: bool,
    pub own: bool,
    pub text: String,
    pub thread: Option<String>,
    pub verified: Option<bool>,
    pub run_id: Option<RunId>,
    pub retired: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub item: KnowledgeItem,
    pub label: ResultLabel,
    pub rank: f64,
    pub reply: Option<KnowledgeItem>,
}

/// Printed "chat-history", "entry", "agent-note".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultLabel {
    ChatHistory,
    Entry,
    AgentNote,
}

impl ResultLabel {
    pub(crate) fn of(kind: ItemKind) -> ResultLabel {
        match kind {
            ItemKind::ChatHistory => ResultLabel::ChatHistory,
            ItemKind::Entries => ResultLabel::Entry,
            ItemKind::Notes => ResultLabel::AgentNote,
        }
    }

    pub(crate) fn text(self) -> &'static str {
        match self {
            ResultLabel::ChatHistory => "chat-history",
            ResultLabel::Entry => "entry",
            ResultLabel::AgentNote => "agent-note",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sender {
    pub id: String,
    pub name: Option<String>,
    pub from_program: bool,
    pub last_active: chrono::DateTime<chrono::Utc>,
    pub items: u64,
}

/// One page of an ingest of one chat-history source in one scope (SA-RECON-01).
#[derive(Debug, Clone, PartialEq)]
pub struct IngestStep {
    /// Differs from every earlier checkpoint of the same job.
    pub checkpoint: serde_json::Value,
    /// Cumulative over the job.
    pub items_read: u64,
    /// Present only when the chat system reports a total; Slack does not, so `None`.
    pub total: Option<u64>,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeChat {
    pub connection: String,
    pub chat: String,
    pub ingest_enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatHistoryPage {
    pub items: Vec<IngestItem>,
    /// The chat system's own position of the newest message read (Slack `ts`), for the next `oldest`.
    pub newest_position: Option<String>,
    pub next_cursor: Option<String>,
    /// The `origin_id` of each item on the page that starts a thread with replies (ADR-107).
    pub threads: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatHistoryError {
    Unavailable,
    Refused,
}

/// Agent profile constants (wrightrun's, unchanged).
pub const MAX_ITEM_CHARS: usize = 40_000;
pub const MAX_QUERY_CHARS: usize = 500;
pub const MAX_RESULTS: u32 = 50;
/// candidates per source = min(limit × 3, 150)
pub const CANDIDATE_FACTOR: u32 = 3;
pub const LINKS_AND_STEPS_FACTOR: f64 = 1.5;
pub const RESULT_TEXT_CHARS: usize = 1_500;
pub const OPEN_TEXT_CHARS: usize = 20_000;
pub const MAX_ENTRY_CHARS: usize = 20_000;
