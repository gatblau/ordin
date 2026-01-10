//! The block catalogue (2C wire §7, copied verbatim) and the check every block passes before it is
//! drawn (ServiceApi.md *Block catalogue and its check*; ADR-028, ADR-067, ADR-068, ADR-070).

use ordin_types::prelude::*;
use ordin_types::wire::{ContentRef, StartUpSubject};
use serde::{Deserialize, Serialize};

mod check;
mod report;

pub use check::{check_block, check_plan_tool, check_report_tool};
pub use report::report_to_domain;

/// Defined once in `ordin_types::domain::runs` and re-exported here (2C, decision 80: C-23).
pub use ordin_types::domain::runs::BarArrangement;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Message(MessageBlock),
    Question(QuestionBlock),
    Form(FormBlock),
    Documents(DocumentsBlock),
    Preview(PreviewBlock),
    Document(DocumentBlock),
    Changes(ChangesBlock),
    PlanProgress(PlanProgressBlock),
    Table(TableBlock),
    Chart(ChartBlock),
    KeyFigures(KeyFiguresBlock),
    Report(ReportBlock),
    EmailList(EmailListBlock),
    Email(EmailBlock),
    TelegramList(TelegramListBlock),
    /// Amended January 2026 (ADR-102).
    SlackList(SlackListBlock),
    Result(ResultBlock),
    AgentProposal(AgentProposalBlock),
    AgentLibrary(AgentLibraryBlock),
    CrewPlan(CrewPlanBlock),
    Memory(MemoryBlock),
    Knowledge(KnowledgeBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MessageBlock {
    /// Plain text; paragraphs separated by a blank line.
    pub text: String,
    /// Attached results (§5.15: "Ordin's reply, with attached results").
    pub attachments: Vec<ContentRef>,
    /// The recalls of the turn that produced this answer, 0–20; omitted when empty. The screens show
    /// **What Ordin used** from them (`memory.used`) (amended January 2026, ADR-111).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_used: Vec<MemoryRecallId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct QuestionBlock {
    pub question: String,
    pub choices: Vec<String>,
    pub allow_other: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FormBlock {
    pub title: String,
    /// The task's own verb on the submit button ("Compare quotes"); never an approval verb.
    pub verb: String,
    pub target: RunTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub fields: Vec<FormField>,
}

/// The Usage Planner's `UsageEstimate`, restated field for field (2C wire §7); carried by
/// `CrewPlanBlock.estimate`, filled by the Ordin Host.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageEstimate {
    pub band: EstimateBand,
    pub min_turns: u32,
    pub max_turns: u32,
    /// Share of a typical day's use, in per-mille of 250 turns.
    pub typical_day_permille: u32,
    pub based_on: Vec<EstimateBasis>,
    /// True when the band reaches the person's "Ask before work estimated as" choice.
    pub ask_first: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EstimateBand {
    Light,
    Medium,
    Heavy,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EstimateBasis {
    EarlierRuns { count: u32 },
    Limits { max_rounds: u32 },
    Members { count: u32 },
    Steps { count: u32 },
    Documents { count: u32, total_bytes: u64 },
}

// SPEC-GAP-3: 2C wire §7 gives this struct both `deny_unknown_fields` and a `#[serde(flatten)]`
// field, which serde does not support together (every flattened key is then refused). The attribute
// is left off; the block check refuses unknown keys of this object itself (`check.rs`, `KEYS`).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FormField {
    pub name: String,
    pub label: String,
    pub required: bool,
    #[serde(flatten)]
    pub input: FormInput,
    /// Set when Ordin filled the field from the conversation or the workspace.
    pub filled_by_ordin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "input", rename_all = "snake_case")]
pub enum FormInput {
    Text {
        value: Option<String>,
        placeholder: Option<String>,
    },
    Document {
        value: Option<ContentRef>,
    },
    Place {
        value: Option<PlaceId>,
    },
    Choice {
        options: Vec<String>,
        value: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DocumentsBlock {
    pub workspace_id: WorkspaceId,
    pub items: Vec<ContentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PreviewBlock {
    pub item: ContentRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DocumentBlock {
    pub item: ContentRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChangesBlock {
    pub before: ContentRef,
    pub after: ContentRef,
    pub view: ChangeView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeView {
    TextDifferences,
    BeforeAndAfterPages,
    BeforeAndAfterImages,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PlanProgressBlock {
    pub subject: StartUpSubject,
}

/// Arguments of the MCP Front's `plan` tool.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanToolArgs {
    /// State (or restate) the plan; restating makes a new plan version.
    State {
        steps: Vec<String>,
    },
    Mark {
        position: u8,
        mark: AgentStepMark,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentStepMark {
    Now,
    Done,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TableBlock {
    pub title: String,
    pub columns: Vec<TableColumn>,
    /// Row-major; each row has exactly one cell per column, in column order.
    pub rows: Vec<Vec<Cell>>,
    pub source: ReportPartSource,
}

// SPEC-GAP-3: 2C wire §7 gives this struct both `deny_unknown_fields` and a `#[serde(flatten)]`
// field, which serde does not support together (every flattened key is then refused). The attribute
// is left off; the block check refuses unknown keys of this object itself (`check.rs`, `KEYS`).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TableColumn {
    pub name: String,
    #[serde(flatten)]
    pub column_type: ColumnType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ColumnType {
    Text,
    Number {
        decimals: Option<u8>,
    },
    /// ISO 4217 code, uppercase.
    Money {
        currency: String,
    },
    Percentage,
    Date,
    DocumentLink,
}

/// A cell: `null`, a string (text, date as YYYY-MM-DD), a number (number, money, percentage as a
/// fraction 0.08 = 8 %), or a `ContentRef` object (document link).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Cell {
    Empty,
    Number(f64),
    Text(String),
    Link(ContentRef),
}

// SPEC-GAP-3: 2C wire §7 gives this struct both `deny_unknown_fields` and a `#[serde(flatten)]`
// field, which serde does not support together (every flattened key is then refused). The attribute
// is left off; the block check refuses unknown keys of this object itself (`check.rs`, `KEYS`).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ChartBlock {
    pub title: String,
    #[serde(flatten)]
    pub chart: ChartKind,
    /// The data rows, as a table; **Show as table** draws exactly these.
    pub columns: Vec<TableColumn>,
    pub rows: Vec<Vec<Cell>>,
    /// Column name on the x axis (category or value), or the slice label column for pie.
    pub x: String,
    /// One or more value columns; each is a series.
    pub series: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y_title: Option<String>,
    pub source: ReportPartSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "chart", rename_all = "snake_case")]
pub enum ChartKind {
    Bar {
        orientation: BarOrientation,
        arrangement: BarArrangement,
    },
    Line,
    Area,
    Scatter,
    Pie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BarOrientation {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KeyFiguresBlock {
    pub figures: Vec<KeyFigure>,
    pub source: ReportPartSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KeyFigure {
    pub label: String,
    pub value: f64,
    /// A unit word ("tickets") or an ISO 4217 currency code; absent for a bare number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<FigureChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FigureChange {
    /// Name of the earlier value ("Q2"); the change is shown only when this is present.
    pub against: String,
    pub earlier_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReportBlock {
    pub title: String,
    pub as_of: Timestamp,
    pub parts: Vec<ReportPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReportPart {
    pub heading: String,
    pub content: ReportPartContent,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportPartContent {
    Table(TableBlock),
    Chart(ChartBlock),
    KeyFigures(KeyFiguresBlock),
}

/// Arguments of the MCP Front's `report` tool: a Report, or one Table, Chart or Key figures.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportToolArgs {
    Report(ReportBlock),
    Table(TableBlock),
    Chart(ChartBlock),
    KeyFigures(KeyFiguresBlock),
}

/// Renamed from `PartSource` (2C wire §7, decision 80: C-5); `PartSource` is the Agent Model's trait.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportPartSource {
    Documents { documents: Vec<SourceDocument> },
    CompanySystem { system: String, read_at: Timestamp },
    EarlierResult { run_id: RunId },
    WorkedOut { by: String, from: Vec<ContentRef> },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SourceDocument {
    pub item: ContentRef,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EmailListBlock {
    pub workspace_id: WorkspaceId,
    /// The email link the list comes from (entity `EmailLink`).
    pub email_link_id: EmailLinkId,
    /// Mail-system message identifiers; the core fills sender, subject, date, attachments.
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "of", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmailBlock {
    Message {
        workspace_id: WorkspaceId,
        message_id: String,
    },
    Draft {
        draft_id: EmailDraftId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramListBlock {
    pub telegram_link_id: TelegramLinkId,
}

/// Amended January 2026 (ADR-102).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackListBlock {
    pub slack_link_id: SlackLinkId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "of", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultBlock {
    /// What a task produced, with where it is.
    Produced {
        run_id: RunId,
        summary: String,
        items: Vec<ContentRef>,
    },
    /// An offer that waits for an answer; the offers are a closed list.
    Offer {
        offer: ResultOffer,
        run_id: Option<RunId>,
    },
    /// A list to tick (Ready to publish, §5.14).
    Checklist {
        title: String,
        items: Vec<ContentRef>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResultOffer {
    SaveAsWorkflow,
    UpdateTheWorkflow,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentProposalBlock {
    pub agent_repository_id: AgentRepositoryId,
    pub agent: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentLibraryBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CrewPlanBlock {
    pub crew_charter_id: CrewCharterId,
    /// "Estimated use" of the crew before Start (ADR-062, UX-A32), from the Usage Planner's `estimate`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate: Option<UsageEstimate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryBlock {
    pub workspace_id: WorkspaceId,
    pub facts: Vec<MemoryFactId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeBlock {
    pub workspace_id: WorkspaceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<KnowledgePageId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BlockRejection {
    pub code: String,
    /// JSON Pointer to the failing field, e.g. `/parts/2/content/source`.
    pub at: String,
    /// One sentence naming the correct form, for the model.
    pub reason: String,
}

/// What the check needs from the caller to resolve references. Implemented by the desktop and server
/// programs, each over the File Service, Mail Service, Journal, Telegram Service (`resolve_list_block`
/// behind `entity`) and Knowledge Service (`resolve_block` behind `entity`).
#[async_trait::async_trait]
pub trait RefResolver: Send + Sync {
    async fn content(&self, workspace_id: WorkspaceId, item: &ContentRef) -> bool;
    async fn email_message(
        &self,
        workspace_id: WorkspaceId,
        email_link_id: EmailLinkId,
        message_id: &str,
    ) -> bool;
    async fn entity(&self, workspace_id: WorkspaceId, entity: EntityRef) -> bool;
    /// The display name of the agent of `run_id`, for `ReportPartSource::WorkedOut.by`.
    async fn run_agent_name(&self, run_id: RunId) -> Option<String>;
    /// Plan versions already recorded for `run_id`, and the current plan's length.
    async fn plan_state(&self, run_id: RunId) -> PlanState;
    /// The Approval Service's approval verbs, which a form's `verb` may not use.
    fn approval_verbs(&self) -> &[String];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityRef {
    Run(RunId),
    Draft(EmailDraftId),
    TelegramLink(TelegramLinkId),
    SlackLink(SlackLinkId),
    CrewCharter(CrewCharterId),
    MemoryFact(MemoryFactId),
    KnowledgePage(KnowledgePageId),
    AgentRepository(AgentRepositoryId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlanState {
    pub versions: u32,
    pub current_len: u8,
}

/// Who sends the block, for the source and context rules.
#[derive(Debug, Clone)]
pub struct BlockCheckContext {
    pub workspace_id: WorkspaceId,
    /// The run producing it; `None` for Ordin's own session.
    pub run_id: Option<RunId>,
    pub now: Timestamp,
}

#[derive(Debug, Clone)]
pub struct CheckedReport {
    pub block: ReportToolArgs,
    pub dropped_parts: Vec<BlockRejection>,
}

/// The block check as one injectable trait (SA-RECON-05). Called by the MCP Front (`report`, `plan`
/// tools) and the Ordin Host (`show` and the blocks of its own session); [`StandardBlockCheck`] is the
/// one implementation and calls the three free functions.
#[async_trait::async_trait]
pub trait BlockCatalogueCheck: Send + Sync {
    async fn check_block(
        &self,
        raw: &serde_json::Value,
        ctx: &BlockCheckContext,
        refs: &dyn RefResolver,
    ) -> Result<Block, Vec<BlockRejection>>;
    async fn check_report_tool(
        &self,
        raw: &serde_json::Value,
        ctx: &BlockCheckContext,
        refs: &dyn RefResolver,
    ) -> Result<CheckedReport, Vec<BlockRejection>>;
    async fn check_plan_tool(
        &self,
        raw: &serde_json::Value,
        run_id: RunId,
        refs: &dyn RefResolver,
    ) -> Result<PlanToolArgs, Vec<BlockRejection>>;
}

/// The one implementation of [`BlockCatalogueCheck`]; holds no state.
#[derive(Debug, Clone, Copy, Default)]
pub struct StandardBlockCheck;

// spec: ordin-desktop/components/ServiceApi#SA-RECON-05
#[async_trait::async_trait]
impl BlockCatalogueCheck for StandardBlockCheck {
    async fn check_block(
        &self,
        raw: &serde_json::Value,
        ctx: &BlockCheckContext,
        refs: &dyn RefResolver,
    ) -> Result<Block, Vec<BlockRejection>> {
        check_block(raw, ctx, refs).await
    }

    async fn check_report_tool(
        &self,
        raw: &serde_json::Value,
        ctx: &BlockCheckContext,
        refs: &dyn RefResolver,
    ) -> Result<CheckedReport, Vec<BlockRejection>> {
        check_report_tool(raw, ctx, refs).await
    }

    async fn check_plan_tool(
        &self,
        raw: &serde_json::Value,
        run_id: RunId,
        refs: &dyn RefResolver,
    ) -> Result<PlanToolArgs, Vec<BlockRejection>> {
        check_plan_tool(raw, run_id, refs).await
    }
}
