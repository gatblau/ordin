//! What Ordin remembers — restated from `MemoryService.md` (SA-RECON-02).
//!
//! The six original `memory.*` rows, `memory.keep_checked` (ADR-103), the thirteen agentic-memory rows
//! (ADR-111) and `memory.more_found` (ADR-113), with every field the amendments add to `FactView`,
//! `ForgetReceipt` and `MemoryUsedView`. A command marked *place* in the owner's table takes exactly one
//! of `workspace_id` and `scope_id`; both or neither is refused at `/input`.

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

pub use ordin_types::wire::knowledge_query::{
    KnowledgeHitKind, PageStatus, PageTier, SharedIdentity, UnansweredSource,
};

use super::rules;
use crate::api::{CommandInput, InputRejection};

/// Facts a `memory.get` asks for (ServiceApi.md row, 1–100).
const GET_IDS_MAX: usize = 100;
/// Recall ids a `memory.used` asks for (ServiceApi.md row, 1–20).
const USED_IDS_MAX: usize = 20;
/// `memory.things` search text (ServiceApi.md row, 1–200 characters).
const SEARCH_MAX_CHARS: usize = 200;
/// A fact's statement: one plain sentence of 1–500 characters (owner Error Table, `MEM_STATEMENT_INVALID`).
const STATEMENT_MAX_CHARS: usize = 500;
/// A link kind: 1–64 characters (owner's fixed constants, ADR-111).
const LINK_KIND_MAX_CHARS: usize = 64;

// ---- views ----

/// One fact as the person sees it (11 §5.10), with the fields of every amendment.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FactView {
    pub id: MemoryFactId,
    pub statement: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// None = all workspaces ("All workspaces", 11 §5.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    pub came_from: FactOrigin,
    pub written_at: Timestamp,
    pub may_be_out_of_date: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_id: Option<KnowledgePageId>,
    /// The heading of the page section the fact was indexed from; set only with `page_id` (ADR-104).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_section: Option<String>,
    /// Recall only: 0–1000, the memory component's final score × 1000, rounded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relevance_permille: Option<u16>,
    /// The first sharer's name, for a fact got from a team folder or already shared (ADR-103).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_at: Option<Timestamp>,
    /// The fact may hold personal data and waits for the person's check before it is shared (ADR-103).
    pub needs_check: bool,
    /// The folder the fact belongs to (ADR-110).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The description of the deepest matching path of that folder (data, never instructions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_description: Option<String>,
    // SPEC-GAP-13: the owner serialises `set_by_company` exactly when `folder` is present, through a
    // private view struct; a restated type read from that JSON needs `default` for the absent key, and
    // writes the field always.
    /// True for a fact from a folder the organisation profile names (ADR-110).
    #[serde(default)]
    pub set_by_company: bool,
    /// "owner/name › path" of the read folder an adopted fact came from (ADR-110).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adopted_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adoption_mark: Option<AdoptionMark>,
    /// Happened, known or summary (ADR-111).
    pub kind: MemoryKind,
    /// `valid_from`.
    pub true_from: Timestamp,
    /// `valid_to`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub true_until: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces: Option<MemoryFactId>,
    pub fading: bool,
    /// Set only when fading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_used_since: Option<Timestamp>,
    /// At most 16.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub things: Vec<ThingRef>,
    /// Summary only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub made_from_count: Option<u16>,
    /// Absent for facts not from a page or a concept (ADR-112).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_tier: Option<PageTier>,
    /// Some only for draft and deprecated (SA-MKA-02).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_status: Option<PageStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
    KnowledgePage {
        page_id: KnowledgePageId,
    },
    Person,
}

/// Where an adopted fact stands against its source (ADR-110).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionMark {
    ChangedAtSource,
    RemovedAtSource,
}

/// episodic | semantic | consolidated (ADR-111).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Happened,
    Known,
    Summary,
}

/// SA-PAGE-01 shape.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FactPage {
    pub items: Vec<FactView>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ForgetReceipt {
    pub id: MemoryFactId,
    pub forgotten_at: Timestamp,
    pub proof_digest: String,
    /// Always serialised; 0 when none (ADR-111).
    pub summaries_removed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ThingRef {
    pub id: MemoryThingId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkBy {
    Agent { agent_instance_id: AgentInstanceId },
    Ordin,
    Person,
    Page { page_id: KnowledgePageId },
    Team,
}

/// The knowledge page that explains a thing.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PageLabel {
    pub page_id: KnowledgePageId,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ThingPage {
    pub items: Vec<ThingView>,
    pub next_cursor: Option<String>,
}

/// At most 200 facts and 200 links.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ThingDetail {
    pub thing: ThingView,
    pub facts: Vec<FactView>,
    pub links: Vec<LinkView>,
}

/// Oldest first.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FactHistoryView {
    pub versions: Vec<FactView>,
    pub links: Vec<LinkView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OriginsView {
    pub items: Vec<OriginItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OriginItem {
    pub fact: FactView,
    pub state: OriginState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OriginState {
    Current,
    NoLongerTrue,
}

/// **What Ordin used**: `gone` counts the items forgotten since.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MemoryUsedView {
    pub facts: Vec<FactView>,
    pub links: Vec<LinkView>,
    pub gone: u16,
    /// The knowledge agents' sources the recall used, 0–50 (ADR-113).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wider: Vec<WiderUsed>,
}

/// One source from a knowledge agent that a recall used; never the outside statement (SA-MKA-07).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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

/// One outside fact from a knowledge agent (ADR-113).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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

/// The late outside facts of one recall, held 30 minutes after it (SA-MKA-07).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MoreFoundView {
    pub recall_id: MemoryRecallId,
    pub wider: Vec<WiderFact>,
    pub unanswered: Vec<UnansweredSource>,
    pub done: bool,
    pub expired: bool,
}

/// Oldest first, at most 500.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RememberedInRun {
    pub items: Vec<RememberedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RememberedItem {
    pub fact: FactView,
    pub by: RememberedBy,
}

/// SA-AMEM-07.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RememberedBy {
    Ordin,
    Agent { agent_instance_id: AgentInstanceId },
    CrewMember { agent_instance_id: AgentInstanceId },
}

/// The numbers the **Forget** confirmation shows; sent back as `memory.forget_thing`'s `expected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ThingCount {
    pub facts: u32,
    pub links: u32,
    pub summaries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ThingForgetReceipt {
    pub id: MemoryThingId,
    pub facts_removed: u32,
    pub links_removed: u32,
    pub summaries_removed: u32,
    pub forgotten_at: Timestamp,
    pub proof_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LinkForgetReceipt {
    pub id: MemoryLinkId,
    pub forgotten_at: Timestamp,
    pub proof_digest: String,
}

/// The summaries a **Forget** takes with it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ForgetCount {
    pub summaries: u32,
}

// ---- inputs ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryListInput {
    pub workspace_id: WorkspaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryGetInput {
    pub workspace_id: WorkspaceId,
    /// 1–100.
    pub ids: Vec<MemoryFactId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MemoryGetResult {
    pub facts: Vec<FactView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryCorrectInput {
    pub id: MemoryFactId,
    /// One plain sentence, 1–500 characters.
    pub statement: String,
}

/// `{ id }`, the input of `memory.forget`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryForgetInput {
    pub id: MemoryFactId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryRememberEverywhereInput {
    pub id: MemoryFactId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryKeepCheckedInput {
    pub id: MemoryFactId,
}

/// *place*, `search?` (1–200), `cursor?`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryThingsInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryThingInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    pub id: MemoryThingId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryFactHistoryInput {
    pub id: MemoryFactId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryFactOriginsInput {
    pub id: MemoryFactId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryKeepInput {
    pub id: MemoryFactId,
}

/// *place*, `recall_ids` (1–20).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryUsedInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    pub recall_ids: Vec<MemoryRecallId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryRememberedInRunInput {
    pub run_id: RunId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryCorrectLinkInput {
    pub id: MemoryLinkId,
    /// 1–64 characters.
    pub kind: String,
    pub from: MemoryThingId,
    pub to: MemoryThingId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryForgetLinkInput {
    pub id: MemoryLinkId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryThingCountInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    pub id: MemoryThingId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryForgetThingInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    pub id: MemoryThingId,
    /// The numbers the person confirmed; refused `MEM_THING_COUNT_CHANGED` by the owner when they moved.
    pub expected: ThingCount,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemorySameAsInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    pub keep: MemoryThingId,
    pub merge: MemoryThingId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryForgetCountInput {
    pub id: MemoryFactId,
}

/// `memory.more_found` (ADR-113).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryMoreFoundInput {
    pub workspace_id: WorkspaceId,
    pub recall_id: MemoryRecallId,
}

// ---- checks ----

/// Inputs whose only fields are identifiers: the typed read already applied every rule.
macro_rules! no_rules {
    ($($ty:ty),* $(,)?) => {$(
        impl CommandInput for $ty {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

no_rules!(
    MemoryForgetInput,
    MemoryRememberEverywhereInput,
    MemoryKeepCheckedInput,
    MemoryFactHistoryInput,
    MemoryFactOriginsInput,
    MemoryKeepInput,
    MemoryRememberedInRunInput,
    MemoryForgetLinkInput,
    MemoryForgetCountInput,
    MemoryMoreFoundInput,
);

impl CommandInput for MemoryListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor_text(&self.cursor, "/cursor")
    }
}

impl CommandInput for MemoryGetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::count(&self.ids, 1, GET_IDS_MAX, "/ids")
    }
}

impl CommandInput for MemoryCorrectInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.statement, 1, STATEMENT_MAX_CHARS, "/statement")
    }
}

impl CommandInput for MemoryThingsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::exactly_one(&self.workspace_id, &self.scope_id, "")?;
        rules::opt_text(&self.search, 1, SEARCH_MAX_CHARS, "/search")?;
        rules::cursor_text(&self.cursor, "/cursor")
    }
}

impl CommandInput for MemoryThingInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::exactly_one(&self.workspace_id, &self.scope_id, "")
    }
}

impl CommandInput for MemoryUsedInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::exactly_one(&self.workspace_id, &self.scope_id, "")?;
        rules::count(&self.recall_ids, 1, USED_IDS_MAX, "/recall_ids")
    }
}

impl CommandInput for MemoryCorrectLinkInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.kind, 1, LINK_KIND_MAX_CHARS, "/kind")
    }
}

impl CommandInput for MemoryThingCountInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::exactly_one(&self.workspace_id, &self.scope_id, "")
    }
}

impl CommandInput for MemoryForgetThingInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::exactly_one(&self.workspace_id, &self.scope_id, "")
    }
}

impl CommandInput for MemorySameAsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::exactly_one(&self.workspace_id, &self.scope_id, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WS: &str = "0192a0f1-1111-7222-8333-444455556666";
    const SCOPE: &str = "0192a0f1-2222-7333-8444-555566667777";
    const FACT: &str = "0192a2d0-1c3e-7a10-8b2f-5d6e7f809a1b";
    const THING: &str = "entity:7f6e5d4c-3b2a-4190-8f7e-6d5c4b3a2910";
    const THING2: &str = "entity:7f6e5d4c-3b2a-4190-8f7e-6d5c4b3a2911";
    const LINK: &str = "relationship:7f6e5d4c-3b2a-4190-8f7e-6d5c4b3a2912";

    /// Reads `value` as `T` and runs its check; the pointer of the refusal, or None when it passes.
    fn refused<T: CommandInput>(value: serde_json::Value) -> Option<(&'static str, String)> {
        let mut input: T = serde_json::from_value(value).expect("shape");
        input.check().err().map(|r| (r.code, r.at))
    }

    fn invalid(at: &str) -> Option<(&'static str, String)> {
        Some(("SVC_INPUT_INVALID", at.to_owned()))
    }

    #[test]
    fn place_is_exactly_one_of_workspace_and_scope() {
        let thing = |extra: serde_json::Value| {
            let mut v = json!({ "id": THING });
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            v
        };
        assert_eq!(
            refused::<MemoryThingInput>(thing(json!({ "workspace_id": WS }))),
            None
        );
        assert_eq!(
            refused::<MemoryThingInput>(thing(json!({ "scope_id": SCOPE }))),
            None
        );
        assert_eq!(
            refused::<MemoryThingInput>(thing(json!({ "workspace_id": WS, "scope_id": SCOPE }))),
            invalid("/input")
        );
        assert_eq!(
            refused::<MemoryThingInput>(thing(json!({}))),
            invalid("/input")
        );
        assert_eq!(
            refused::<MemoryThingCountInput>(thing(json!({}))),
            invalid("/input")
        );
        assert_eq!(
            refused::<MemoryForgetThingInput>(thing(
                json!({ "expected": { "facts": 1, "links": 0, "summaries": 0 } })
            )),
            invalid("/input")
        );
        assert_eq!(
            refused::<MemorySameAsInput>(
                json!({ "workspace_id": WS, "keep": THING, "merge": THING2 })
            ),
            None
        );
        assert_eq!(
            refused::<MemorySameAsInput>(json!({ "keep": THING, "merge": THING2 })),
            invalid("/input")
        );
    }

    #[test]
    fn things_search_and_cursor() {
        assert_eq!(
            refused::<MemoryThingsInput>(json!({ "workspace_id": WS, "search": "Borealis" })),
            None
        );
        assert_eq!(
            refused::<MemoryThingsInput>(json!({ "workspace_id": WS, "scope_id": SCOPE })),
            invalid("/input")
        );
        assert_eq!(
            refused::<MemoryThingsInput>(json!({ "workspace_id": WS, "search": "" })),
            invalid("/input/search")
        );
        assert_eq!(
            refused::<MemoryThingsInput>(json!({ "workspace_id": WS, "search": "x".repeat(201) })),
            invalid("/input/search")
        );
        assert_eq!(
            refused::<MemoryThingsInput>(json!({ "workspace_id": WS, "cursor": "c".repeat(1025) })),
            Some(("SRV_BAD_CURSOR", "/input/cursor".to_owned()))
        );
        assert_eq!(
            refused::<MemoryListInput>(json!({ "workspace_id": WS, "cursor": "abc" })),
            None
        );
        assert_eq!(
            refused::<MemoryListInput>(json!({ "workspace_id": WS, "cursor": "c".repeat(1025) })),
            Some(("SRV_BAD_CURSOR", "/input/cursor".to_owned()))
        );
    }

    #[test]
    fn counts_and_lengths() {
        assert_eq!(
            refused::<MemoryGetInput>(json!({ "workspace_id": WS, "ids": [FACT] })),
            None
        );
        assert_eq!(
            refused::<MemoryGetInput>(json!({ "workspace_id": WS, "ids": [] })),
            invalid("/input/ids")
        );
        assert_eq!(
            refused::<MemoryGetInput>(json!({ "workspace_id": WS, "ids": vec![FACT; 101] })),
            invalid("/input/ids")
        );
        let recall = "0192a1c2-3344-7556-8677-8899aabbccdd";
        assert_eq!(
            refused::<MemoryUsedInput>(json!({ "workspace_id": WS, "recall_ids": [recall] })),
            None
        );
        assert_eq!(
            refused::<MemoryUsedInput>(
                json!({ "scope_id": SCOPE, "recall_ids": vec![recall; 21] })
            ),
            invalid("/input/recall_ids")
        );
        assert_eq!(
            refused::<MemoryUsedInput>(json!({ "recall_ids": [recall] })),
            invalid("/input")
        );
        assert_eq!(
            refused::<MemoryCorrectInput>(json!({ "id": FACT, "statement": "Kestrel moved." })),
            None
        );
        assert_eq!(
            refused::<MemoryCorrectInput>(json!({ "id": FACT, "statement": "x".repeat(501) })),
            invalid("/input/statement")
        );
        let link = |kind: String| json!({ "id": LINK, "kind": kind, "from": THING, "to": THING2 });
        assert_eq!(
            refused::<MemoryCorrectLinkInput>(link("supplies".into())),
            None
        );
        assert_eq!(
            refused::<MemoryCorrectLinkInput>(link(String::new())),
            invalid("/input/kind")
        );
        assert_eq!(
            refused::<MemoryCorrectLinkInput>(link("k".repeat(65))),
            invalid("/input/kind")
        );
    }

    #[test]
    fn identifier_only_inputs_pass() {
        assert_eq!(refused::<MemoryForgetInput>(json!({ "id": FACT })), None);
        assert_eq!(
            refused::<MemoryRememberEverywhereInput>(json!({ "id": FACT })),
            None
        );
        assert_eq!(
            refused::<MemoryKeepCheckedInput>(json!({ "id": FACT })),
            None
        );
        assert_eq!(
            refused::<MemoryFactHistoryInput>(json!({ "id": FACT })),
            None
        );
        assert_eq!(
            refused::<MemoryFactOriginsInput>(json!({ "id": FACT })),
            None
        );
        assert_eq!(refused::<MemoryKeepInput>(json!({ "id": FACT })), None);
        assert_eq!(
            refused::<MemoryForgetCountInput>(json!({ "id": FACT })),
            None
        );
        assert_eq!(
            refused::<MemoryForgetLinkInput>(json!({ "id": LINK })),
            None
        );
        assert_eq!(
            refused::<MemoryRememberedInRunInput>(
                json!({ "run_id": "0192a1c2-3344-7556-8677-8899aabbccdd" })
            ),
            None
        );
        assert_eq!(
            refused::<MemoryMoreFoundInput>(
                json!({ "workspace_id": WS, "recall_id": "0192a1c2-3344-7556-8677-8899aabbccdd" })
            ),
            None
        );
    }

    #[test]
    fn malformed_thing_id_is_a_shape_error() {
        let read = serde_json::from_value::<MemoryThingInput>(
            json!({ "workspace_id": WS, "id": "Borealis" }),
        );
        assert!(read.is_err());
        let unknown =
            serde_json::from_value::<MemoryForgetInput>(json!({ "id": FACT, "force": true }));
        assert!(unknown.is_err());
    }

    #[test]
    fn fact_view_reads_the_owners_minimal_json() {
        let view: FactView = serde_json::from_value(json!({
            "id": FACT, "statement": "Kestrel's Q3 quote is valid until 30 November.",
            "came_from": { "kind": "person" }, "written_at": "2026-01-10T09:12:44.318Z",
            "may_be_out_of_date": false, "needs_check": false, "kind": "known",
            "true_from": "2026-01-10T09:12:44.318Z", "fading": false
        }))
        .expect("fact view");
        assert!(!view.set_by_company);
        assert!(view.things.is_empty());
        let out = serde_json::to_value(&view).expect("serialise");
        assert!(out.get("things").is_none());
        assert!(out.get("folder").is_none());
        assert_eq!(out["came_from"]["kind"], "person");
    }
}
