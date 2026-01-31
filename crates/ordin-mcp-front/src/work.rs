//! The MCP Front's own provider: server `work` with the tools `report`, `plan` and `read_rest`
//! (McpFront.md *The MCP Front's own provider* and Internal Logic F).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use ordin_journal::{Journal, JournalError, PlannedStep};
use ordin_service_api::blocks::{
    report_to_domain, AgentStepMark, Block, BlockCheckContext, BlockRejection, PlanToolArgs,
    ReportToolArgs,
};
use ordin_service_api::blocks::{BlockCatalogueCheck, RefResolver};
use ordin_service_api::ServiceEvent;
use ordin_types::domain::runs::StepSource;
use ordin_types::time::Timestamp;
use ordin_types::wire::events::EventKind;
use ordin_types::wire::run::StepMark;
use ordin_types::{BlockId, CodedError, ReportId, RunId};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::names::ServerName;
use crate::provider::{
    ArgumentProblem, ServerSpec, ShownBlockStore, ToolCallContext, ToolFailure, ToolKey,
    ToolOutput, ToolProvider, ToolSpec,
};
use crate::types::{ProgramProfile, SessionGrant};
use crate::util::{clip, lock, pointer_token};

/// The `provider_id` of the built-in provider.
// A registration of the same id is refused as a duplicate.
pub(crate) const WORK_PROVIDER_ID: &str = "mcp-front-work";
pub(crate) const WORK_SERVER: &str = "work";

/// A block event whose serialised data exceeds this is announced by id (SA-RECON-07).
const MAX_EVENT_DATA_BYTES: usize = 256 * 1024;
/// A block event whose serialised data exceeds this is kept for `ordin.block`: 1 KiB below the
/// announcement limit, so a frame the transport replaces for its envelope is kept too
/// (SA-MCP-BLOCK-01).
const KEEP_ABOVE_BYTES: usize = 255 * 1024;

/// Arguments of `read_rest`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadRestArgs {
    reference: String,
    from: u64,
}

/// A failure of a `work` call: the ordinary failure plus the structured part some carry.
pub(crate) struct WorkFailure {
    pub(crate) failure: ToolFailure,
    pub(crate) structured: Option<Value>,
}

impl WorkFailure {
    fn plain(code: &str, message: impl Into<String>, retryable: bool) -> Self {
        WorkFailure {
            failure: ToolFailure {
                code: code.to_owned(),
                message: message.into(),
                retryable,
            },
            structured: None,
        }
    }
}

/// The plan tracker of one run (Internal Logic 26.3).
#[derive(Debug, Default, Clone)]
struct PlanTracker {
    version: u32,
    steps: u8,
    now: Option<u8>,
}

/// Trimmed remainders of one session, oldest evicted first (Internal Logic 19).
#[derive(Default)]
struct RestStore {
    entries: VecDeque<RestEntry>,
    bytes: usize,
}

struct RestEntry {
    reference: String,
    /// Character offset of the first stored character within the whole output.
    base: u64,
    text: String,
}

#[derive(Default)]
struct WorkSession {
    plan: PlanTracker,
    rest: RestStore,
}

/// State of the `work` tools, shared between the pipeline and the provider.
pub(crate) struct WorkShared {
    journal: Journal,
    blocks: Arc<dyn BlockCatalogueCheck>,
    resolver: OnceLock<Arc<dyn RefResolver>>,
    /// Where a block announced by id is kept (SA-MCP-BLOCK-02).
    block_store: OnceLock<Arc<dyn ShownBlockStore>>,
    only_needed: AtomicBool,
    trim_keep_chars: usize,
    max_rest_bytes: usize,
    sessions: Mutex<HashMap<RunId, WorkSession>>,
}

impl WorkShared {
    pub(crate) fn new(
        journal: Journal,
        blocks: Arc<dyn BlockCatalogueCheck>,
        trim_keep_chars: usize,
        max_rest_bytes: usize,
    ) -> Self {
        WorkShared {
            journal,
            blocks,
            resolver: OnceLock::new(),
            block_store: OnceLock::new(),
            only_needed: AtomicBool::new(false),
            trim_keep_chars,
            max_rest_bytes,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn bind_resolver(&self, resolver: Arc<dyn RefResolver>) -> bool {
        self.resolver.set(resolver).is_ok()
    }

    pub(crate) fn bind_block_store(&self, store: Arc<dyn ShownBlockStore>) -> bool {
        self.block_store.set(store).is_ok()
    }

    pub(crate) fn set_only_needed(&self, on: bool) {
        self.only_needed.store(on, Ordering::SeqCst);
    }

    pub(crate) fn only_needed(&self) -> bool {
        self.only_needed.load(Ordering::SeqCst)
    }

    /// Creates the session's state, rebuilding the plan tracker from the Journal when the run
    /// already has a plan (a resumed run, Internal Logic 26.3).
    pub(crate) async fn open(&self, run_id: RunId) {
        let mut tracker = PlanTracker::default();
        match self.journal.current_plan(run_id).await {
            Ok(steps) if !steps.is_empty() => {
                tracker.version = steps.first().map(|s| s.plan_version).unwrap_or(0);
                tracker.steps = u8::try_from(steps.len()).unwrap_or(u8::MAX);
                tracker.now = steps
                    .iter()
                    .find(|s| s.mark == StepMark::Now)
                    .and_then(|s| u8::try_from(s.position).ok());
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(
                component = "mcp_front",
                run_id = %run_id,
                code = CodedError::code(&error),
                "mcp_front.plan_rebuild_failed"
            ),
        }
        lock(&self.sessions).insert(
            run_id,
            WorkSession {
                plan: tracker,
                rest: RestStore::default(),
            },
        );
    }

    pub(crate) fn close(&self, run_id: RunId) {
        lock(&self.sessions).remove(&run_id);
    }

    /// Stores the remainder of a trimmed output and returns its reference.
    pub(crate) fn store_rest(&self, run_id: RunId, base: u64, remainder: String) -> Option<String> {
        let mut sessions = lock(&self.sessions);
        let session = sessions.get_mut(&run_id)?;
        let mut text = remainder;
        if text.len() > self.max_rest_bytes {
            let mut cut = self.max_rest_bytes;
            while cut > 0 && !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        let reference = format!("rest:{}", uuid::Uuid::now_v7());
        session.rest.bytes += text.len();
        session.rest.entries.push_back(RestEntry {
            reference: reference.clone(),
            base,
            text,
        });
        while session.rest.bytes > self.max_rest_bytes {
            match session.rest.entries.pop_front() {
                Some(old) => session.rest.bytes = session.rest.bytes.saturating_sub(old.text.len()),
                None => break,
            }
        }
        Some(reference)
    }

    fn tracker(&self, run_id: RunId) -> PlanTracker {
        lock(&self.sessions)
            .get(&run_id)
            .map(|s| s.plan.clone())
            .unwrap_or_default()
    }

    fn set_tracker(&self, run_id: RunId, tracker: PlanTracker) {
        if let Some(session) = lock(&self.sessions).get_mut(&run_id) {
            session.plan = tracker;
        }
    }

    /// Runs one `work` call (Internal Logic F).
    pub(crate) async fn run(
        &self,
        ctx: &ToolCallContext,
        tool: &str,
        arguments: Value,
    ) -> Result<ToolOutput, WorkFailure> {
        match tool {
            "report" => self.report(ctx, arguments).await,
            "plan" => self.plan(ctx, arguments).await,
            "read_rest" => self.read_rest(ctx, arguments),
            // The offered check admits only the three tools, so this arm is not reached.
            _ => Err(WorkFailure::plain(
                "MCP_TOOL_NOT_OFFERED",
                "This tool is not offered to this session.",
                false,
            )),
        }
    }

    fn resolver(&self, text: &'static str) -> Result<&Arc<dyn RefResolver>, WorkFailure> {
        self.resolver.get().ok_or_else(|| unbound(text))
    }

    /// Step 25.2.2: hands a block above `KEEP_ABOVE_BYTES` to the bound store.
    // spec: ordin-desktop/components/McpFront#SA-MCP-BLOCK-02
    async fn keep(
        &self,
        ctx: &ToolCallContext,
        block_id: BlockId,
        block: &Block,
    ) -> Result<(), WorkFailure> {
        let Some(store) = self.block_store.get() else {
            tracing::error!(
                component = "mcp_front",
                run_id = %ctx.run_id,
                correlation_id = %ctx.correlation_id,
                "mcp_front.block_store_missing"
            );
            return Err(WorkFailure::plain(
                "MCP_BLOCK_STORE_NOT_BOUND",
                "This is too large to show yet; show it as smaller tables or charts.",
                false,
            ));
        };
        store
            .keep_shown_block(ctx.workspace_id, block_id, block)
            .await
            .map_err(|failure| {
                tracing::warn!(
                    component = "mcp_front",
                    run_id = %ctx.run_id,
                    correlation_id = %ctx.correlation_id,
                    code = failure.code.as_str(),
                    "mcp_front.block_keep_failed"
                );
                WorkFailure {
                    failure,
                    structured: None,
                }
            })
    }

    async fn report(
        &self,
        ctx: &ToolCallContext,
        arguments: Value,
    ) -> Result<ToolOutput, WorkFailure> {
        let refs = self.resolver("Reports can't be shown yet.")?;
        let check_ctx = BlockCheckContext {
            workspace_id: ctx.workspace_id,
            run_id: Some(ctx.run_id),
            now: Timestamp::now(),
        };
        let checked = match self
            .blocks
            .check_report_tool(&arguments, &check_ctx, refs.as_ref())
            .await
        {
            Ok(checked) => checked,
            Err(rejections) => return Err(report_refused(rejections)),
        };
        let journal_failed = |_: JournalError| {
            tracing::error!(
                component = "mcp_front",
                run_id = %ctx.run_id,
                correlation_id = %ctx.correlation_id,
                code = "MCP_JOURNAL_FAILED",
                "mcp_front.journal_failed"
            );
            WorkFailure::plain(
                "MCP_JOURNAL_FAILED",
                "The report could not be saved; try again.",
                true,
            )
        };
        // spec: ordin-desktop/components/McpFront#SA-MCP-BLOCK-03
        // Step 25.2 as amended: the block is built and, when large, kept before anything is
        // stored in the Journal, so a refusal leaves nothing behind and a retry starts clean.
        let (title, block) = match &checked.block {
            ReportToolArgs::Report(report) => (report.title.clone(), Block::Report(report.clone())),
            ReportToolArgs::Table(table) => (table.title.clone(), Block::Table(table.clone())),
            ReportToolArgs::Chart(chart) => (chart.title.clone(), Block::Chart(chart.clone())),
            ReportToolArgs::KeyFigures(figures) => {
                ("Key figures".to_owned(), Block::KeyFigures(figures.clone()))
            }
        };
        let block_id = BlockId::new();
        let shown = ServiceEvent::BlockShown {
            workspace_id: ctx.workspace_id,
            block_id,
            block,
            author: None,
        };
        let mut data = shown.to_journal_data();
        let size = serde_json::to_vec(&data)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        // spec: ordin-desktop/components/McpFront#SA-MCP-BLOCK-01
        if let (true, ServiceEvent::BlockShown { block, .. }) = (size > KEEP_ABOVE_BYTES, &shown) {
            self.keep(ctx, block_id, block).await?;
        }
        let mut report_id: Option<ReportId> = None;
        if let ReportToolArgs::Report(report) = &checked.block {
            let id = ReportId::new();
            self.journal
                .store_report(report_to_domain(report, id, ctx.workspace_id, ctx.run_id))
                .await
                .map_err(|e| match e {
                    JournalError::Invalid {
                        code: code @ "REPORT_RUN_HAS_REPORT",
                    } => WorkFailure::plain(
                        code,
                        "This run already has its report. Show anything more as a table, a \
                         chart or key figures.",
                        false,
                    ),
                    other => journal_failed(other),
                })?;
            report_id = Some(id);
        }
        // spec: ordin-desktop/components/McpFront#SA-RECON-07
        let mut event = shown;
        if size > MAX_EVENT_DATA_BYTES {
            event = ServiceEvent::BlockAvailable {
                workspace_id: ctx.workspace_id,
                block_id,
            };
            data = event.to_journal_data();
        }
        let kind = match event {
            ServiceEvent::BlockAvailable { .. } => EventKind::BlockAvailable,
            _ => EventKind::BlockShown,
        };
        self.journal
            .append_event(kind.as_str(), data)
            .await
            .map_err(journal_failed)?;
        let left_out = checked.dropped_parts;
        let mut text = format!("Shown: {title}.");
        if !left_out.is_empty() {
            text.push_str(&format!(
                " {} part(s) left out; see left_out.",
                left_out.len()
            ));
        }
        let mut structured = json!({ "block_id": block_id, "left_out": left_out });
        if let (Some(id), Some(map)) = (report_id, structured.as_object_mut()) {
            map.insert("report_id".to_owned(), json!(id));
        }
        Ok(ToolOutput {
            text,
            structured: Some(structured),
        })
    }

    async fn plan(
        &self,
        ctx: &ToolCallContext,
        arguments: Value,
    ) -> Result<ToolOutput, WorkFailure> {
        let refs = self.resolver("The plan can't be recorded yet.")?;
        let args = match self
            .blocks
            .check_plan_tool(&arguments, ctx.run_id, refs.as_ref())
            .await
        {
            Ok(args) => args,
            Err(rejections) => {
                let first = rejections.into_iter().next();
                let (code, reason) = match first {
                    Some(r) => (r.code, r.reason),
                    None => (
                        "BLOCK_PLAN_INVALID".to_owned(),
                        "The plan is not valid.".to_owned(),
                    ),
                };
                return Err(WorkFailure::plain(&code, clip(&reason, 300), false));
            }
        };
        let journal_failed = |_: JournalError| {
            tracing::error!(
                component = "mcp_front",
                run_id = %ctx.run_id,
                correlation_id = %ctx.correlation_id,
                code = "MCP_JOURNAL_FAILED",
                "mcp_front.journal_failed"
            );
            WorkFailure::plain(
                "MCP_JOURNAL_FAILED",
                "The plan could not be saved; try again.",
                true,
            )
        };
        match args {
            PlanToolArgs::State { steps } => {
                let count = steps.len();
                let planned = steps
                    .into_iter()
                    .map(|line| PlannedStep {
                        line,
                        came_from: StepSource::AgentsPlan,
                    })
                    .collect();
                let stored = self
                    .journal
                    .state_plan(ctx.run_id, planned)
                    .await
                    .map_err(journal_failed)?;
                let version = stored.first().map(|s| s.plan_version).unwrap_or(0);
                self.set_tracker(
                    ctx.run_id,
                    PlanTracker {
                        version,
                        steps: u8::try_from(count).unwrap_or(u8::MAX),
                        now: None,
                    },
                );
                Ok(ToolOutput {
                    text: format!("Plan noted: {count} steps."),
                    structured: Some(json!({ "plan_version": version })),
                })
            }
            PlanToolArgs::Mark { position, mark } => {
                let mut tracker = self.tracker(ctx.run_id);
                if tracker.steps == 0 {
                    return Err(WorkFailure::plain(
                        "MCP_PLAN_NOT_STATED",
                        "State your steps first with plan(action: \"state\").",
                        false,
                    ));
                }
                let one_now = || {
                    WorkFailure::plain(
                        "MCP_PLAN_ONE_NOW",
                        format!("Mark step {position} done or skipped first."),
                        false,
                    )
                };
                if mark == AgentStepMark::Now && tracker.now.is_some_and(|n| n != position) {
                    return Err(one_now());
                }
                let domain_mark = match mark {
                    AgentStepMark::Now => StepMark::Now,
                    AgentStepMark::Done => StepMark::Done,
                    AgentStepMark::Skipped => StepMark::Skipped,
                };
                match self
                    .journal
                    .mark_step(ctx.run_id, u32::from(position), domain_mark)
                    .await
                {
                    Ok(_) => {}
                    // A Journal refusal of an immutable step is answered as "one now".
                    Err(JournalError::Invalid {
                        code: "RSTEP_IMMUTABLE",
                    }) => return Err(one_now()),
                    Err(other) => return Err(journal_failed(other)),
                }
                if mark == AgentStepMark::Now {
                    tracker.now = Some(position);
                } else if tracker.now == Some(position) {
                    tracker.now = None;
                }
                let version = tracker.version;
                self.set_tracker(ctx.run_id, tracker);
                let word = match mark {
                    AgentStepMark::Now => "now",
                    AgentStepMark::Done => "done",
                    AgentStepMark::Skipped => "skipped",
                };
                Ok(ToolOutput {
                    text: format!("Marked step {position} {word}."),
                    structured: Some(json!({ "plan_version": version })),
                })
            }
        }
    }

    fn read_rest(
        &self,
        ctx: &ToolCallContext,
        arguments: Value,
    ) -> Result<ToolOutput, WorkFailure> {
        let unknown = || {
            WorkFailure::plain(
                "MCP_REST_UNKNOWN",
                "That reference has expired; ask for the document again.",
                false,
            )
        };
        let args: ReadRestArgs = serde_json::from_value(arguments).map_err(|_| unknown())?;
        let sessions = lock(&self.sessions);
        let entry = sessions
            .get(&ctx.run_id)
            .and_then(|s| {
                s.rest
                    .entries
                    .iter()
                    .find(|e| e.reference == args.reference)
            })
            .ok_or_else(unknown)?;
        let total = entry.base + entry.text.chars().count() as u64;
        // An offset before the first stored character cannot be served, because only the
        // remainder is kept; it is refused with the same code.
        if args.from > total || args.from < entry.base {
            let text = if args.from > total {
                format!("That offset is past the end ({total} characters).")
            } else {
                format!(
                    "That offset is before the part kept (it starts at {}).",
                    entry.base
                )
            };
            return Err(WorkFailure::plain("MCP_REST_OFFSET_INVALID", text, false));
        }
        let skip = usize::try_from(args.from - entry.base).unwrap_or(usize::MAX);
        let part: String = entry
            .text
            .chars()
            .skip(skip)
            .take(self.trim_keep_chars)
            .collect();
        let next = args.from + part.chars().count() as u64;
        let tail = if next >= total {
            "[End.]".to_owned()
        } else {
            format!("[Next: from {next}]")
        };
        Ok(ToolOutput {
            text: format!("{part}\n{tail}"),
            structured: None,
        })
    }
}

/// The failure of a `report` or `plan` call made before the program bound its resolver: not
/// retryable (SA-CLOSE-01).
// spec: ordin-desktop/components/McpFront#SA-CLOSE-01
fn unbound(text: &'static str) -> WorkFailure {
    tracing::error!(component = "mcp_front", "mcp_front.ref_resolver_missing");
    WorkFailure::plain("MCP_REF_RESOLVER_NOT_BOUND", text, false)
}

/// `BLOCK_REPORT_REFUSED` with the rejections (at most 20) as structured data.
fn report_refused(rejections: Vec<BlockRejection>) -> WorkFailure {
    let reason = rejections
        .first()
        .map(|r| clip(&r.reason, 300))
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| "The report could not be shown.".to_owned());
    let listed: Vec<BlockRejection> = rejections.into_iter().take(20).collect();
    WorkFailure {
        failure: ToolFailure {
            code: "BLOCK_REPORT_REFUSED".to_owned(),
            message: reason,
            retryable: false,
        },
        structured: Some(json!({ "rejections": listed })),
    }
}

/// A schema's root must be `"type": "object"`; a tagged enum's schema is a `oneOf`, so the type
/// is added at the root.
fn object_schema<T: schemars::JsonSchema>() -> Value {
    let mut value = serde_json::to_value(schemars::schema_for!(T)).unwrap_or_else(|_| json!({}));
    if let Some(map) = value.as_object_mut() {
        flatten_root_variants(map);
        map.entry("type").or_insert_with(|| json!("object"));
    }
    value
}

/// Merges a tagged enum's root `oneOf` into one object. The model API refuses a tool whose schema
/// has `oneOf` at the root, and a harness then drops the tool silently. Only the published
/// description changes: the arguments are still parsed into the enum, and the root description
/// says which fields each variant needs.
// spec: ordin-desktop/components/McpFront#SA-RECON-10
fn flatten_root_variants(root: &mut serde_json::Map<String, Value>) {
    let Some(Value::Array(branches)) = root.remove("oneOf") else {
        return;
    };
    let defs = root.get("$defs").cloned().unwrap_or(Value::Null);
    let mut properties = serde_json::Map::new();
    let mut required: Option<Vec<Value>> = None;
    let mut notes = Vec::new();
    for branch in &branches {
        let (fields, needed) = branch_fields(branch, &defs);
        let mut tag = None;
        for (name, schema) in fields {
            if let Some(value) = schema.get("const") {
                tag = Some(format!("{name} = {}", value.as_str().unwrap_or_default()));
                merge_tag(&mut properties, &name, value);
            } else {
                merge_field(&mut properties, name, schema);
            }
        }
        if let Some(tag) = tag {
            let names: Vec<&str> = needed.iter().filter_map(Value::as_str).collect();
            notes.push(format!("{tag} needs {}.", names.join(", ")));
        }
        required = Some(match required {
            None => needed,
            Some(held) => held.into_iter().filter(|n| needed.contains(n)).collect(),
        });
    }
    let summary = root.get("description").and_then(Value::as_str);
    let description = summary.map_or(notes.join(" "), |s| format!("{s} {}", notes.join(" ")));
    root.insert("description".to_owned(), json!(description));
    root.insert("properties".to_owned(), Value::Object(properties));
    root.insert("required".to_owned(), json!(required.unwrap_or_default()));
    root.insert("additionalProperties".to_owned(), json!(false));
}

/// A variant's fields and required names, following a `$ref` a newtype variant carries.
fn branch_fields(branch: &Value, defs: &Value) -> (serde_json::Map<String, Value>, Vec<Value>) {
    let mut fields = serde_json::Map::new();
    let mut needed = Vec::new();
    let target = branch
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/$defs/"))
        .and_then(|name| defs.get(name));
    for part in target.into_iter().chain([branch]) {
        if let Some(props) = part.get("properties").and_then(Value::as_object) {
            fields.extend(props.clone());
        }
        if let Some(names) = part.get("required").and_then(Value::as_array) {
            needed.extend(names.iter().cloned());
        }
    }
    (fields, needed)
}

fn merge_tag(properties: &mut serde_json::Map<String, Value>, name: &str, value: &Value) {
    let entry = properties
        .entry(name.to_owned())
        .or_insert_with(|| json!({"type": "string", "enum": []}));
    if let Some(values) = entry.get_mut("enum").and_then(Value::as_array_mut) {
        values.push(value.clone());
    }
}

fn merge_field(properties: &mut serde_json::Map<String, Value>, name: String, schema: Value) {
    match properties.get_mut(&name) {
        None => {
            properties.insert(name, schema);
        }
        Some(held) if *held == schema => {}
        Some(held) => {
            if let Some(options) = held.get_mut("anyOf").and_then(Value::as_array_mut) {
                if !options.contains(&schema) {
                    options.push(schema);
                }
            } else {
                *held = json!({"anyOf": [held.clone(), schema]});
            }
        }
    }
}

/// The built-in provider.
pub(crate) struct WorkProvider {
    pub(crate) shared: Arc<WorkShared>,
}

#[async_trait]
impl ToolProvider for WorkProvider {
    fn provider_id(&self) -> &'static str {
        WORK_PROVIDER_ID
    }

    fn servers(&self) -> Vec<ServerSpec> {
        let Ok(name) = ServerName::new(WORK_SERVER) else {
            return Vec::new();
        };
        vec![ServerSpec {
            name,
            tools: vec![
                ToolSpec {
                    name: "report".to_owned(),
                    description: "Show a report, table, chart or key figures to the person."
                        .to_owned(),
                    input_schema: object_schema::<ReportToolArgs>(),
                    argument_form: "report(kind: \"report\"|\"table\"|\"chart\"|\"key_figures\", …the block's fields)".to_owned(),
                    sandbox_callable: false,
                    plain: "Can show you reports, tables and charts.".to_owned(),
                },
                ToolSpec {
                    name: "plan".to_owned(),
                    description:
                        "State your 3–7 steps before other tools, then mark each now or done."
                            .to_owned(),
                    input_schema: object_schema::<PlanToolArgs>(),
                    argument_form: "plan(action: \"state\", steps: [3–7 lines of ≤80 characters]) or plan(action: \"mark\", position: 1–7, mark: \"now\"|\"done\"|\"skipped\")".to_owned(),
                    sandbox_callable: false,
                    plain: "Tells you its steps as it works.".to_owned(),
                },
                ToolSpec {
                    name: "read_rest".to_owned(),
                    description: "Read more of an output that was cut short.".to_owned(),
                    input_schema: object_schema::<ReadRestArgs>(),
                    argument_form:
                        "read_rest(reference: \"rest:<id>\", from: <offset>)".to_owned(),
                    sandbox_callable: true,
                    plain: "Reads long documents in parts.".to_owned(),
                },
            ],
        }]
    }

    // spec: ordin-desktop/components/McpFront#SA-RECON-04
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> {
        let Ok(server) = ServerName::new(WORK_SERVER) else {
            return Ok(Vec::new());
        };
        let key = |tool: &str| ToolKey {
            server: server.clone(),
            tool: tool.to_owned(),
        };
        let mut keys = Vec::new();
        // In the agent profile only `read_rest` is offered (assumed in the spec, Internal Logic 23).
        if matches!(
            grant.profile,
            ProgramProfile::Desktop | ProgramProfile::PersonalServer
        ) {
            keys.push(key("report"));
            keys.push(key("plan"));
        }
        if self.shared.only_needed() {
            keys.push(key("read_rest"));
        }
        Ok(keys)
    }

    fn check_arguments(&self, key: &ToolKey, arguments: &Value) -> Result<(), ArgumentProblem> {
        match key.tool.as_str() {
            "report" => typed::<ReportToolArgs>(arguments).map(|_| ()),
            "plan" => {
                let args = typed::<PlanToolArgs>(arguments)?;
                // The correct form names positions 1 to 7.
                if let PlanToolArgs::Mark { position, .. } = args {
                    if !(1..=7).contains(&position) {
                        return Err(ArgumentProblem {
                            at: "/position".to_owned(),
                            reason: "the position must be from 1 to 7".to_owned(),
                        });
                    }
                }
                Ok(())
            }
            "read_rest" => typed::<ReadRestArgs>(arguments).map(|_| ()),
            _ => Err(ArgumentProblem {
                at: String::new(),
                reason: "unknown tool".to_owned(),
            }),
        }
    }

    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        self.shared
            .run(&ctx, &key.tool, arguments)
            .await
            .map_err(|failure| failure.failure)
    }

    async fn session_closed(&self, run_id: RunId) {
        self.shared.close(run_id);
    }
}

/// Typed deserialisation with unknown fields refused; the serde path becomes the JSON Pointer.
// spec: ordin-desktop/components/McpFront#SA-API-02
fn typed<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, ArgumentProblem> {
    serde_path_to_error::deserialize::<_, T>(value).map_err(|error| {
        let mut at = String::new();
        for segment in error.path().iter() {
            match segment {
                serde_path_to_error::Segment::Seq { index } => {
                    at.push_str(&format!("/{index}"));
                }
                serde_path_to_error::Segment::Map { key } => {
                    at.push('/');
                    at.push_str(&pointer_token(key));
                }
                serde_path_to_error::Segment::Enum { .. }
                | serde_path_to_error::Segment::Unknown => {}
            }
        }
        let message = error.inner().to_string();
        for prefix in ["unknown field `", "missing field `"] {
            if let Some(rest) = message.strip_prefix(prefix) {
                if let Some(field) = rest.split('`').next() {
                    let suffix = format!("/{}", pointer_token(field));
                    if !at.ends_with(&suffix) {
                        at.push_str(&suffix);
                    }
                }
            }
        }
        ArgumentProblem {
            at,
            reason: clip(&message, 300),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_names_the_failing_field() {
        let err = typed::<ReadRestArgs>(&json!({"reference": "rest:x", "from": "no"})).unwrap_err();
        assert_eq!(err.at, "/from");
        let err =
            typed::<ReadRestArgs>(&json!({"reference": "r", "from": 1, "extra": 1})).unwrap_err();
        assert_eq!(err.at, "/extra");
        let err = typed::<ReadRestArgs>(&json!({"reference": "r"})).unwrap_err();
        assert_eq!(err.at, "/from");
        assert!(typed::<ReadRestArgs>(&json!({"reference": "r", "from": 0})).is_ok());
    }

    #[test]
    fn an_unbound_resolver_fails_without_retry() {
        let failure = unbound("Reports can't be shown yet.");
        assert_eq!(failure.failure.code, "MCP_REF_RESOLVER_NOT_BOUND");
        assert_eq!(failure.failure.message, "Reports can't be shown yet.");
        assert!(!failure.failure.retryable);
    }

    #[test]
    fn schemas_are_objects() {
        assert_eq!(object_schema::<ReportToolArgs>()["type"], "object");
        assert_eq!(object_schema::<PlanToolArgs>()["type"], "object");
        assert_eq!(object_schema::<ReadRestArgs>()["type"], "object");
    }

    #[test]
    fn schemas_have_no_variants_at_the_root() {
        let report = object_schema::<ReportToolArgs>();
        assert!(report.get("oneOf").is_none());
        assert_eq!(
            report["properties"]["kind"]["enum"],
            json!(["report", "table", "chart", "key_figures"])
        );
        assert_eq!(report["required"], json!(["kind"]));
        // The chart variant is a newtype, so its fields sit behind a `$ref`.
        let figures = report["description"].as_str().unwrap_or_default();
        assert!(figures.contains("kind = key_figures needs kind, figures, source."));
        assert!(figures.contains("kind = chart needs"));
        assert!(report["properties"]
            .as_object()
            .is_some_and(|p| p.len() > 7));

        let plan = object_schema::<PlanToolArgs>();
        assert!(plan.get("oneOf").is_none());
        assert_eq!(
            plan["properties"]["action"]["enum"],
            json!(["state", "mark"])
        );
        assert_eq!(plan["required"], json!(["action"]));
    }

    #[test]
    fn refused_report_lists_at_most_twenty() {
        let rejections: Vec<BlockRejection> = (0..25)
            .map(|i| BlockRejection {
                code: "BLOCK_FIELD_UNKNOWN".to_owned(),
                at: format!("/x{i}"),
                reason: "Use the correct form.".to_owned(),
            })
            .collect();
        let failure = report_refused(rejections);
        assert_eq!(failure.failure.code, "BLOCK_REPORT_REFUSED");
        assert_eq!(failure.failure.message, "Use the correct form.");
        let listed = failure.structured.unwrap()["rejections"]
            .as_array()
            .unwrap()
            .len();
        assert_eq!(listed, 20);
    }
}
