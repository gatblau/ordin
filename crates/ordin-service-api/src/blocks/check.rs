//! The block check (Internal Logic 10–12): size, shape, per-kind rules, then reference resolution.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures::StreamExt;
use ordin_types::prelude::*;
use ordin_types::text::normalise_text;
use ordin_types::wire::{ContentRef, StartUpSubject};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::*;

/// Largest serialised block (2C wire §7, A-34).
pub(crate) const MAX_BLOCK_BYTES: usize = 2 * 1024 * 1024;
/// Rejections returned per call.
const MAX_REJECTIONS: usize = 20;
/// Reference checks running at once (10.4).
const RESOLVE_CONCURRENCY: usize = 16;
/// The bound on one reference check (10.4); a timeout counts as unresolved.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(2);
/// How far in the future a report's `as_of` may be.
const AS_OF_SLACK_MILLIS: i64 = 5 * 60 * 1000;
/// Verbs a form may never carry besides the Approval Service's (10.6).
const FORBIDDEN_VERBS: [&str; 3] = ["ok", "yes", "continue"];
/// The block kinds, as `kind` spells them.
const KINDS: [&str; 22] = [
    "message",
    "question",
    "form",
    "documents",
    "preview",
    "document",
    "changes",
    "plan_progress",
    "table",
    "chart",
    "key_figures",
    "report",
    "email_list",
    "email",
    "telegram_list",
    "slack_list",
    "result",
    "agent_proposal",
    "agent_library",
    "crew_plan",
    "memory",
    "knowledge",
];

/// One fixed English sentence per code, naming the correct form (10.7; for the model, not the person).
fn reason(code: &str) -> &'static str {
    match code {
        "BLOCK_TOO_LARGE" => "A block may be at most 2 MiB of JSON; show less, or split it.",
        "BLOCK_KIND_UNKNOWN" => "The kind must be one of the block catalogue's kinds, for example message, table or report.",
        "BLOCK_FIELD_UNKNOWN" => "This field is not part of the block's form; send only the catalogue's fields with their types.",
        "BLOCK_LINK_NOT_ALLOWED" => "Links are references to a document or place in the workspace, never a web address.",
        "BLOCK_TEXT_INVALID" => "This text is empty or longer than the block allows.",
        "BLOCK_TOO_MANY_ITEMS" => "This list is empty or holds more items than the block allows.",
        "BLOCK_CHOICES_INVALID" => "A question is 1 to 300 characters with 2 to 4 different choices of 1 to 60 characters.",
        "BLOCK_FORM_INVALID" => "A form has 1 to 20 uniquely named fields with labels of 1 to 80 characters, and its verb names the task, never an approval.",
        "BLOCK_REF_UNRESOLVED" => "This reference does not name something the person can read in this workspace.",
        "BLOCK_PATH_INVALID" => "A relative path is 1 to 1,024 bytes, with no leading slash and no '..' segment.",
        "BLOCK_PLAN_INVALID" => "A plan states 3 to 7 steps of 1 to 80 characters each.",
        "BLOCK_PLAN_POSITION_INVALID" => "The position must name a step of the current plan, counting from 1.",
        "BLOCK_PLAN_TOO_MANY_VERSIONS" => "A run may state its plan at most 20 times.",
        "BLOCK_TABLE_INVALID" => "A table has a title of 1 to 120 characters and uniquely named columns of 1 to 60 characters.",
        "BLOCK_TABLE_TOO_WIDE" => "A table has 1 to 30 columns.",
        "BLOCK_TABLE_TOO_LONG" => "A table has at most 10,000 rows.",
        "BLOCK_CELL_TYPE_MISMATCH" => "Each row has one cell per column, and each cell matches its column's type; dates are YYYY-MM-DD and numbers finite.",
        "BLOCK_CELL_TOO_LONG" => "A text cell is at most 2,000 characters.",
        "BLOCK_CURRENCY_INVALID" => "A money column names its currency as three capital letters (ISO 4217).",
        "BLOCK_SOURCE_MISSING" => "Every part needs a source: documents with versions, a company system with the time read, an earlier result, or who worked it out and from what.",
        "BLOCK_SOURCE_INVALID" => "A source names 1 to 20 documents with versions of 1 to 64 characters, a system of 1 to 80 characters, or the producing agent's own name.",
        "BLOCK_CHART_SERIES_INVALID" => "A chart has 1 to 8 series, each naming a numeric column.",
        "BLOCK_CHART_TOO_MANY_POINTS" => "A chart shows at most 5,000 points (rows times series).",
        "BLOCK_CHART_PIE_INVALID" => "A pie chart has exactly one series and at most 6 rows.",
        "BLOCK_CHART_AXIS_INVALID" => "The x axis names one of the chart's columns, a numeric one for a scatter chart.",
        "BLOCK_CHART_INVALID" => "Chart titles are 1 to 120 characters.",
        "BLOCK_FIGURES_COUNT" => "Key figures hold 1 to 8 figures.",
        "BLOCK_FIGURES_INVALID" => "A figure has a label of 1 to 60 characters, a unit of 1 to 24, a comparison name of 1 to 60, and finite values.",
        "BLOCK_REPORT_PARTS_COUNT" => "A report has 1 to 12 parts.",
        "BLOCK_REPORT_INVALID" => "A report's title and each part's heading are 1 to 120 characters.",
        "BLOCK_AS_OF_INVALID" => "A report's 'as of' time may be at most 5 minutes in the future.",
        "BLOCK_EMAIL_NOT_LINKED" => "List 1 to 200 messages of 1 to 998 characters from the workspace's own email links.",
        "BLOCK_OFFER_UNKNOWN" => "An offer is one of save_as_workflow or update_the_workflow.",
        _ => "This block does not match the block catalogue.",
    }
}

fn rejection(code: &str, at: impl Into<String>) -> BlockRejection {
    BlockRejection {
        code: code.to_owned(),
        at: at.into(),
        reason: reason(code).to_owned(),
    }
}

/// The JSON Pointer of a `serde_path_to_error` path, under `base`.
fn pointer(base: &str, path: &serde_path_to_error::Path) -> String {
    use serde_path_to_error::Segment;
    let mut out = base.to_owned();
    for segment in path.iter() {
        match segment {
            Segment::Seq { index } => out.push_str(&format!("/{index}")),
            Segment::Map { key } => {
                out.push('/');
                out.push_str(&key.replace('~', "~0").replace('/', "~1"));
            }
            Segment::Enum { .. } | Segment::Unknown => {}
        }
    }
    out
}

/// The value at a JSON Pointer of `root`, if any.
fn value_at<'a>(root: &'a Value, ptr: &str) -> Option<&'a Value> {
    root.pointer(ptr)
}

/// The text between the first pair of backticks of a serde message (`unknown field `x``).
fn quoted(message: &str) -> Option<&str> {
    let start = message.find('`')? + 1;
    let end = start + message[start..].find('`')?;
    Some(&message[start..end])
}

/// Deserialises `value` into `T`, turning a shape error into the rejection 10.2 names. `base` is the
/// pointer of `value` inside the whole block; `root` is the whole block, to look at a failing value.
fn parse<T: DeserializeOwned>(
    value: &Value,
    base: &str,
    root: &Value,
) -> Result<T, BlockRejection> {
    serde_path_to_error::deserialize::<_, T>(value).map_err(|error| {
        let at = pointer(base, error.path());
        let message = error.inner().to_string();
        if let Some(field) = message.strip_prefix("unknown field ").and_then(quoted) {
            // The path already ends at the unknown key when serde read it as a map entry.
            let suffix = format!("/{field}");
            if at.ends_with(&suffix) {
                return rejection("BLOCK_FIELD_UNKNOWN", at);
            }
            return rejection("BLOCK_FIELD_UNKNOWN", format!("{at}{suffix}"));
        }
        if let Some(field) = message.strip_prefix("missing field ").and_then(quoted) {
            if field == "source" {
                return rejection("BLOCK_SOURCE_MISSING", format!("{at}/source"));
            }
            return rejection("BLOCK_FIELD_UNKNOWN", format!("{at}/{field}"));
        }
        // An offer outside the closed list; inside the `of`-tagged result the path stops at the
        // block, so the expected variants identify it.
        if message.starts_with("unknown variant") && message.contains("`save_as_workflow`") {
            let at = if at.ends_with("/offer") {
                at
            } else {
                format!("{at}/offer")
            };
            return rejection("BLOCK_OFFER_UNKNOWN", at);
        }
        if let Some(Value::String(s)) = value_at(root, &at) {
            if url::Url::parse(s).is_ok() {
                return rejection("BLOCK_LINK_NOT_ALLOWED", at);
            }
        }
        BlockRejection {
            code: "BLOCK_FIELD_UNKNOWN".to_owned(),
            at,
            reason: message.chars().take(300).collect(),
        }
    })
}

/// The object `value` without its tag key, for reading a tagged variant's struct directly.
fn without_key(value: &Value, key: &str) -> Value {
    match value {
        Value::Object(map) => {
            let mut map = map.clone();
            map.remove(key);
            Value::Object(map)
        }
        other => other.clone(),
    }
}

/// NFC-normalises every string of `value` in place (10.3). A string the shared rule refuses (a
/// control character other than `\n` and `\t`) is left as it is: a block's text is drawn as text.
fn normalise(value: &mut Value) {
    match value {
        Value::String(s) => {
            if let Ok(n) = normalise_text(s) {
                *s = n;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalise),
        Value::Object(map) => map.values_mut().for_each(normalise),
        _ => {}
    }
}

/// The kind of a block's JSON, or `BLOCK_KIND_UNKNOWN` at `/kind`.
fn kind_of(raw: &Value) -> Result<&str, BlockRejection> {
    match raw.get("kind").and_then(Value::as_str) {
        Some(kind) if KINDS.contains(&kind) => Ok(kind),
        _ => Err(rejection("BLOCK_KIND_UNKNOWN", "/kind")),
    }
}

/// The keys of the three block objects that carry a flattened part, which serde cannot refuse unknown
/// keys of (SPEC-GAP-3).
const COLUMN_KEYS: [&str; 4] = ["name", "type", "decimals", "currency"];
const FIELD_KEYS: [&str; 8] = [
    "name",
    "label",
    "required",
    "input",
    "value",
    "placeholder",
    "options",
    "filled_by_ordin",
];
const CHART_KEYS: [&str; 12] = [
    "kind",
    "title",
    "chart",
    "orientation",
    "arrangement",
    "columns",
    "rows",
    "x",
    "series",
    "x_title",
    "y_title",
    "source",
];

/// The first key of `object` outside `allowed`, as a rejection at `<at>/<key>`.
fn unknown_key(object: &Value, allowed: &[&str], at: &str) -> Result<(), BlockRejection> {
    if let Value::Object(map) = object {
        if let Some(key) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(rejection("BLOCK_FIELD_UNKNOWN", format!("{at}/{key}")));
        }
    }
    Ok(())
}

/// Refuses unknown keys of every table column, chart and form field in a block (SPEC-GAP-3).
fn check_flattened_keys(value: &Value, at: &str) -> Result<(), BlockRejection> {
    let Value::Object(map) = value else {
        return Ok(());
    };
    match map.get("kind").and_then(Value::as_str) {
        Some("chart") => unknown_key(value, &CHART_KEYS, at)?,
        Some("form") => {
            if let Some(Value::Array(fields)) = map.get("fields") {
                for (i, field) in fields.iter().enumerate() {
                    unknown_key(field, &FIELD_KEYS, &format!("{at}/fields/{i}"))?;
                }
            }
        }
        Some("report") => {
            if let Some(Value::Array(parts)) = map.get("parts") {
                for (i, part) in parts.iter().enumerate() {
                    if let Some(content) = part.get("content") {
                        check_flattened_keys(content, &format!("{at}/parts/{i}/content"))?;
                    }
                }
            }
        }
        _ => {}
    }
    if let Some(Value::Array(columns)) = map.get("columns") {
        for (i, column) in columns.iter().enumerate() {
            unknown_key(column, &COLUMN_KEYS, &format!("{at}/columns/{i}"))?;
        }
    }
    Ok(())
}

/// Reads a whole block, variant by variant so that every shape error keeps its path.
fn parse_block(raw: &Value) -> Result<Block, BlockRejection> {
    let kind = kind_of(raw)?;
    check_flattened_keys(raw, "")?;
    let body = without_key(raw, "kind");
    let block = match kind {
        "message" => Block::Message(parse(&body, "", raw)?),
        "question" => Block::Question(parse(&body, "", raw)?),
        "form" => Block::Form(parse(&body, "", raw)?),
        "documents" => Block::Documents(parse(&body, "", raw)?),
        "preview" => Block::Preview(parse(&body, "", raw)?),
        "document" => Block::Document(parse(&body, "", raw)?),
        "changes" => Block::Changes(parse(&body, "", raw)?),
        "plan_progress" => Block::PlanProgress(parse(&body, "", raw)?),
        "table" => Block::Table(parse(&body, "", raw)?),
        "chart" => Block::Chart(parse(&body, "", raw)?),
        "key_figures" => Block::KeyFigures(parse(&body, "", raw)?),
        "report" => Block::Report(parse_report(raw)?),
        "email_list" => Block::EmailList(parse(&body, "", raw)?),
        "email" => Block::Email(parse(&body, "", raw)?),
        "telegram_list" => Block::TelegramList(parse(&body, "", raw)?),
        "slack_list" => Block::SlackList(parse(&body, "", raw)?),
        "result" => Block::Result(parse(&body, "", raw)?),
        "agent_proposal" => Block::AgentProposal(parse(&body, "", raw)?),
        "agent_library" => Block::AgentLibrary(parse(&body, "", raw)?),
        "crew_plan" => Block::CrewPlan(parse(&body, "", raw)?),
        "memory" => Block::Memory(parse(&body, "", raw)?),
        _ => Block::Knowledge(parse(&body, "", raw)?),
    };
    Ok(block)
}

/// The envelope fields of a report, read without its parts.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportEnvelope {
    title: String,
    as_of: Timestamp,
    parts: Vec<Value>,
}

/// Reads a report, part by part, so that a failing part's pointer is `/parts/<i>/…`.
fn parse_report(raw: &Value) -> Result<ReportBlock, BlockRejection> {
    let envelope: ReportEnvelope = parse(&without_key(raw, "kind"), "", raw)?;
    let mut parts = Vec::with_capacity(envelope.parts.len());
    for (i, part) in envelope.parts.iter().enumerate() {
        parts.push(parse_part(part, &format!("/parts/{i}"), raw)?);
    }
    Ok(ReportBlock {
        title: envelope.title,
        as_of: envelope.as_of,
        parts,
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PartEnvelope {
    heading: String,
    content: Value,
}

/// Reads one report part at `base`.
fn parse_part(part: &Value, base: &str, root: &Value) -> Result<ReportPart, BlockRejection> {
    let envelope: PartEnvelope = parse(part, base, root)?;
    let at = format!("{base}/content");
    check_flattened_keys(&envelope.content, &at)?;
    let body = without_key(&envelope.content, "kind");
    let content = match envelope.content.get("kind").and_then(Value::as_str) {
        Some("table") => ReportPartContent::Table(parse(&body, &at, root)?),
        Some("chart") => ReportPartContent::Chart(parse(&body, &at, root)?),
        Some("key_figures") => ReportPartContent::KeyFigures(parse(&body, &at, root)?),
        _ => return Err(rejection("BLOCK_KIND_UNKNOWN", format!("{at}/kind"))),
    };
    Ok(ReportPart {
        heading: envelope.heading,
        content,
    })
}

/// Collects rejections up to the per-call limit.
#[derive(Default)]
struct Found(Vec<BlockRejection>);

impl Found {
    fn add(&mut self, code: &str, at: impl Into<String>) {
        if self.0.len() < MAX_REJECTIONS {
            self.0.push(rejection(code, at));
        }
    }

    fn text(&mut self, value: &str, min: usize, max: usize, code: &str, at: impl Into<String>) {
        let n = value.chars().count();
        if n < min || n > max {
            self.add(code, at);
        }
    }

    fn count(&mut self, len: usize, min: usize, max: usize, code: &str, at: impl Into<String>) {
        if len < min || len > max {
            self.add(code, at);
        }
    }
}

/// One reference to resolve, with where it sits and the code a failure carries.
enum Pending {
    Content(ContentRef),
    Email(EmailLinkId, String),
    Entity(EntityRef),
}

struct Refs {
    items: Vec<(Pending, String, &'static str)>,
}

impl Refs {
    fn content(&mut self, found: &mut Found, item: &ContentRef, at: &str) {
        if let ContentRef::PlaceFile { relative_path, .. } = item {
            let bad = relative_path.is_empty()
                || relative_path.len() > 1024
                || relative_path.starts_with('/')
                || relative_path.split(['/', '\\']).any(|s| s == "..");
            if bad {
                found.add("BLOCK_PATH_INVALID", format!("{at}/relative_path"));
                return;
            }
        }
        self.items.push((
            Pending::Content(item.clone()),
            at.to_owned(),
            "BLOCK_REF_UNRESOLVED",
        ));
    }

    fn entity(&mut self, entity: EntityRef, at: &str) {
        self.items.push((
            Pending::Entity(entity),
            at.to_owned(),
            "BLOCK_REF_UNRESOLVED",
        ));
    }
}

/// True when a cell's column takes numbers.
fn numeric(column: &ColumnType) -> bool {
    matches!(
        column,
        ColumnType::Number { .. } | ColumnType::Money { .. } | ColumnType::Percentage
    )
}

/// `YYYY-MM-DD`, a real calendar date.
fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| text[r].chars().all(|c| c.is_ascii_digit());
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let year: i32 = text[0..4].parse().unwrap_or(0);
    let month: u32 = text[5..7].parse().unwrap_or(0);
    let day: u32 = text[8..10].parse().unwrap_or(0);
    chrono_like_valid(year, month, day)
}

/// A Gregorian date check without a date library.
fn chrono_like_valid(year: i32, month: u32, day: u32) -> bool {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// Columns and cells, shared by tables and charts (the *table* and *table cell* rows).
fn check_grid(
    found: &mut Found,
    refs: &mut Refs,
    columns: &[TableColumn],
    rows: &[Vec<Cell>],
    base: &str,
) {
    found.count(
        columns.len(),
        1,
        30,
        "BLOCK_TABLE_TOO_WIDE",
        format!("{base}/columns"),
    );
    found.count(
        rows.len(),
        0,
        10_000,
        "BLOCK_TABLE_TOO_LONG",
        format!("{base}/rows"),
    );
    for (i, column) in columns.iter().enumerate() {
        let at = format!("{base}/columns/{i}");
        found.text(
            &column.name,
            1,
            60,
            "BLOCK_TABLE_INVALID",
            format!("{at}/name"),
        );
        if columns[..i].iter().any(|c| c.name == column.name) {
            found.add("BLOCK_TABLE_INVALID", format!("{at}/name"));
        }
        if let ColumnType::Money { currency } = &column.column_type {
            let ok = currency.len() == 3 && currency.chars().all(|c| c.is_ascii_uppercase());
            if !ok {
                found.add("BLOCK_CURRENCY_INVALID", format!("{at}/currency"));
            }
        }
    }
    for (r, row) in rows.iter().enumerate() {
        if row.len() != columns.len() {
            // *chosen here*: 2C says each row has one cell per column; a short or long row is a
            // cell that does not match its column.
            found.add("BLOCK_CELL_TYPE_MISMATCH", format!("{base}/rows/{r}"));
            continue;
        }
        for (c, (cell, column)) in row.iter().zip(columns).enumerate() {
            let at = format!("{base}/rows/{r}/{c}");
            let matches = match (cell, &column.column_type) {
                (Cell::Empty, _) => true,
                (Cell::Number(v), t) if numeric(t) => v.is_finite(),
                (Cell::Text(s), ColumnType::Text) => {
                    if s.chars().count() > 2_000 {
                        found.add("BLOCK_CELL_TOO_LONG", at.clone());
                    }
                    true
                }
                (Cell::Text(s), ColumnType::Date) => is_date(s),
                (Cell::Link(item), ColumnType::DocumentLink) => {
                    refs.content(found, item, &at);
                    true
                }
                _ => false,
            };
            if !matches {
                found.add("BLOCK_CELL_TYPE_MISMATCH", at);
            }
        }
    }
}

/// The *part source* row, and the source's references.
fn check_source(
    found: &mut Found,
    refs: &mut Refs,
    source: &ReportPartSource,
    base: &str,
    agent_name: &Option<String>,
    run_known: bool,
) {
    let at = format!("{base}/source");
    match source {
        ReportPartSource::Documents { documents } => {
            found.count(
                documents.len(),
                1,
                20,
                "BLOCK_SOURCE_INVALID",
                format!("{at}/documents"),
            );
            for (i, doc) in documents.iter().enumerate() {
                let d = format!("{at}/documents/{i}");
                found.text(
                    &doc.version,
                    1,
                    64,
                    "BLOCK_SOURCE_INVALID",
                    format!("{d}/version"),
                );
                refs.content(found, &doc.item, &format!("{d}/item"));
            }
        }
        ReportPartSource::CompanySystem { system, .. } => {
            found.text(
                system,
                1,
                80,
                "BLOCK_SOURCE_INVALID",
                format!("{at}/system"),
            );
        }
        ReportPartSource::EarlierResult { run_id } => {
            refs.entity(EntityRef::Run(*run_id), &format!("{at}/run_id"));
        }
        ReportPartSource::WorkedOut { by, from } => {
            found.text(by, 1, 80, "BLOCK_SOURCE_INVALID", format!("{at}/by"));
            // 10.5: `by` names the producing run's agent when the block comes from a run.
            if run_known && agent_name.as_deref() != Some(by.as_str()) {
                found.add("BLOCK_SOURCE_INVALID", format!("{at}/by"));
            }
            for (i, item) in from.iter().enumerate() {
                refs.content(found, item, &format!("{at}/from/{i}"));
            }
        }
    }
}

/// The context the per-kind rules need beyond the block.
struct RuleContext<'a> {
    now: Timestamp,
    agent_name: Option<String>,
    run_known: bool,
    approval_verbs: &'a [String],
}

fn check_table(found: &mut Found, refs: &mut Refs, t: &TableBlock, base: &str, rc: &RuleContext) {
    found.text(
        &t.title,
        1,
        120,
        "BLOCK_TABLE_INVALID",
        format!("{base}/title"),
    );
    check_grid(found, refs, &t.columns, &t.rows, base);
    check_source(found, refs, &t.source, base, &rc.agent_name, rc.run_known);
}

fn check_chart(found: &mut Found, refs: &mut Refs, c: &ChartBlock, base: &str, rc: &RuleContext) {
    found.text(
        &c.title,
        1,
        120,
        "BLOCK_CHART_INVALID",
        format!("{base}/title"),
    );
    if let Some(t) = &c.x_title {
        found.text(t, 1, 120, "BLOCK_CHART_INVALID", format!("{base}/x_title"));
    }
    if let Some(t) = &c.y_title {
        found.text(t, 1, 120, "BLOCK_CHART_INVALID", format!("{base}/y_title"));
    }
    check_grid(found, refs, &c.columns, &c.rows, base);
    let column = |name: &str| c.columns.iter().find(|col| col.name == name);
    let series_ok = (1..=8).contains(&c.series.len())
        && c.series
            .iter()
            .all(|s| column(s).is_some_and(|col| numeric(&col.column_type)));
    if !series_ok {
        found.add("BLOCK_CHART_SERIES_INVALID", format!("{base}/series"));
    }
    if c.rows.len().saturating_mul(c.series.len()) > 5_000 {
        found.add("BLOCK_CHART_TOO_MANY_POINTS", format!("{base}/rows"));
    }
    if c.chart == ChartKind::Pie {
        if c.series.len() != 1 {
            found.add("BLOCK_CHART_PIE_INVALID", format!("{base}/series"));
        } else if c.rows.len() > 6 {
            found.add("BLOCK_CHART_PIE_INVALID", format!("{base}/rows"));
        }
    }
    match column(&c.x) {
        None => found.add("BLOCK_CHART_AXIS_INVALID", format!("{base}/x")),
        Some(col) if c.chart == ChartKind::Scatter && !numeric(&col.column_type) => {
            found.add("BLOCK_CHART_AXIS_INVALID", format!("{base}/x"))
        }
        Some(_) => {}
    }
    check_source(found, refs, &c.source, base, &rc.agent_name, rc.run_known);
}

fn check_figures(
    found: &mut Found,
    refs: &mut Refs,
    k: &KeyFiguresBlock,
    base: &str,
    rc: &RuleContext,
) {
    found.count(
        k.figures.len(),
        1,
        8,
        "BLOCK_FIGURES_COUNT",
        format!("{base}/figures"),
    );
    for (i, figure) in k.figures.iter().enumerate() {
        let at = format!("{base}/figures/{i}");
        found.text(
            &figure.label,
            1,
            60,
            "BLOCK_FIGURES_INVALID",
            format!("{at}/label"),
        );
        if let Some(unit) = &figure.unit {
            found.text(unit, 1, 24, "BLOCK_FIGURES_INVALID", format!("{at}/unit"));
        }
        if !figure.value.is_finite() {
            found.add("BLOCK_FIGURES_INVALID", format!("{at}/value"));
        }
        if let Some(change) = &figure.change {
            found.text(
                &change.against,
                1,
                60,
                "BLOCK_FIGURES_INVALID",
                format!("{at}/change/against"),
            );
            if !change.earlier_value.is_finite() {
                found.add(
                    "BLOCK_FIGURES_INVALID",
                    format!("{at}/change/earlier_value"),
                );
            }
        }
    }
    check_source(found, refs, &k.source, base, &rc.agent_name, rc.run_known);
}

fn check_part(found: &mut Found, refs: &mut Refs, part: &ReportPart, base: &str, rc: &RuleContext) {
    found.text(
        &part.heading,
        1,
        120,
        "BLOCK_REPORT_INVALID",
        format!("{base}/heading"),
    );
    let at = format!("{base}/content");
    match &part.content {
        ReportPartContent::Table(t) => check_table(found, refs, t, &at, rc),
        ReportPartContent::Chart(c) => check_chart(found, refs, c, &at, rc),
        ReportPartContent::KeyFigures(k) => check_figures(found, refs, k, &at, rc),
    }
}

/// The report's own rules (title, `as_of`, part count), without its parts.
fn check_report_envelope(found: &mut Found, r: &ReportBlock, rc: &RuleContext) {
    found.text(&r.title, 1, 120, "BLOCK_REPORT_INVALID", "/title");
    if r.as_of.unix_millis() > rc.now.unix_millis() + AS_OF_SLACK_MILLIS {
        found.add("BLOCK_AS_OF_INVALID", "/as_of");
    }
    found.count(r.parts.len(), 1, 12, "BLOCK_REPORT_PARTS_COUNT", "/parts");
}

/// The per-kind rules of one whole block (10.3, 10.5, 10.6), collecting references for 10.4.
fn check_rules(found: &mut Found, refs: &mut Refs, block: &Block, rc: &RuleContext) {
    match block {
        Block::Message(m) => {
            found.text(&m.text, 1, 8_000, "BLOCK_TEXT_INVALID", "/text");
            found.count(
                m.attachments.len(),
                0,
                20,
                "BLOCK_TOO_MANY_ITEMS",
                "/attachments",
            );
            let distinct = m
                .memory_used
                .iter()
                .enumerate()
                .all(|(i, id)| !m.memory_used[..i].contains(id));
            if m.memory_used.len() > 20 || !distinct {
                found.add("BLOCK_TOO_MANY_ITEMS", "/memory_used");
            }
            for (i, item) in m.attachments.iter().enumerate() {
                refs.content(found, item, &format!("/attachments/{i}"));
            }
        }
        Block::Question(q) => {
            found.text(&q.question, 1, 300, "BLOCK_CHOICES_INVALID", "/question");
            found.count(q.choices.len(), 2, 4, "BLOCK_CHOICES_INVALID", "/choices");
            for (i, choice) in q.choices.iter().enumerate() {
                found.text(
                    choice,
                    1,
                    60,
                    "BLOCK_CHOICES_INVALID",
                    format!("/choices/{i}"),
                );
                if q.choices[..i].contains(choice) {
                    found.add("BLOCK_CHOICES_INVALID", format!("/choices/{i}"));
                }
            }
        }
        Block::Form(f) => check_form(found, refs, f, rc),
        Block::Documents(d) => {
            found.count(d.items.len(), 1, 500, "BLOCK_TOO_MANY_ITEMS", "/items");
            for (i, item) in d.items.iter().enumerate() {
                refs.content(found, item, &format!("/items/{i}"));
            }
        }
        Block::Preview(p) => refs.content(found, &p.item, "/item"),
        Block::Document(d) => refs.content(found, &d.item, "/item"),
        Block::Changes(c) => {
            refs.content(found, &c.before, "/before");
            refs.content(found, &c.after, "/after");
        }
        Block::PlanProgress(p) => {
            if let StartUpSubject::Run { run_id } = &p.subject {
                refs.entity(EntityRef::Run(*run_id), "/subject/run_id");
            }
        }
        Block::Table(t) => check_table(found, refs, t, "", rc),
        Block::Chart(c) => check_chart(found, refs, c, "", rc),
        Block::KeyFigures(k) => check_figures(found, refs, k, "", rc),
        Block::Report(r) => {
            check_report_envelope(found, r, rc);
            for (i, part) in r.parts.iter().enumerate() {
                check_part(found, refs, part, &format!("/parts/{i}"), rc);
            }
        }
        Block::EmailList(e) => {
            found.count(
                e.messages.len(),
                1,
                200,
                "BLOCK_EMAIL_NOT_LINKED",
                "/messages",
            );
            for (i, message) in e.messages.iter().enumerate() {
                let at = format!("/messages/{i}");
                let n = message.chars().count();
                if !(1..=998).contains(&n) {
                    found.add("BLOCK_EMAIL_NOT_LINKED", at);
                } else {
                    refs.items.push((
                        Pending::Email(e.email_link_id, message.clone()),
                        at,
                        "BLOCK_EMAIL_NOT_LINKED",
                    ));
                }
            }
        }
        Block::Email(EmailBlock::Message { message_id, .. }) => {
            // SPEC-GAP-1: `RefResolver` resolves an email message only with its email link, which
            // `EmailBlock::Message` does not carry; the 1–998 length rule is applied, the
            // membership rule cannot be.
            found.text(message_id, 1, 998, "BLOCK_EMAIL_NOT_LINKED", "/message_id");
        }
        Block::Email(EmailBlock::Draft { draft_id }) => {
            refs.entity(EntityRef::Draft(*draft_id), "/draft_id");
        }
        Block::TelegramList(t) => refs.entity(
            EntityRef::TelegramLink(t.telegram_link_id),
            "/telegram_link_id",
        ),
        Block::SlackList(s) => refs.entity(EntityRef::SlackLink(s.slack_link_id), "/slack_link_id"),
        Block::Result(r) => match r {
            ResultBlock::Produced {
                run_id,
                summary,
                items,
            } => {
                found.text(summary, 1, 2_000, "BLOCK_TEXT_INVALID", "/summary");
                refs.entity(EntityRef::Run(*run_id), "/run_id");
                for (i, item) in items.iter().enumerate() {
                    refs.content(found, item, &format!("/items/{i}"));
                }
            }
            ResultBlock::Offer { run_id, .. } => {
                if let Some(run_id) = run_id {
                    refs.entity(EntityRef::Run(*run_id), "/run_id");
                }
            }
            ResultBlock::Checklist { items, .. } => {
                for (i, item) in items.iter().enumerate() {
                    refs.content(found, item, &format!("/items/{i}"));
                }
            }
        },
        Block::AgentProposal(a) => {
            found.text(&a.reason, 1, 300, "BLOCK_TEXT_INVALID", "/reason");
            found.text(&a.agent, 1, 256, "BLOCK_TEXT_INVALID", "/agent");
            refs.entity(
                EntityRef::AgentRepository(a.agent_repository_id),
                "/agent_repository_id",
            );
        }
        Block::AgentLibrary(_) => {}
        Block::CrewPlan(c) => refs.entity(
            EntityRef::CrewCharter(c.crew_charter_id),
            "/crew_charter_id",
        ),
        Block::Memory(m) => {
            found.count(m.facts.len(), 1, 100, "BLOCK_TOO_MANY_ITEMS", "/facts");
            for (i, fact) in m.facts.iter().enumerate() {
                refs.entity(EntityRef::MemoryFact(*fact), &format!("/facts/{i}"));
            }
        }
        Block::Knowledge(k) => {
            if let Some(page) = k.page {
                refs.entity(EntityRef::KnowledgePage(page), "/page");
            }
        }
    }
}

fn check_form(found: &mut Found, refs: &mut Refs, f: &FormBlock, rc: &RuleContext) {
    const CODE: &str = "BLOCK_FORM_INVALID";
    found.count(f.fields.len(), 1, 20, CODE, "/fields");
    for (i, field) in f.fields.iter().enumerate() {
        let at = format!("/fields/{i}");
        let name_ok = field.name.len() <= 32
            && field.name.starts_with(|c: char| c.is_ascii_lowercase())
            && field
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !name_ok || f.fields[..i].iter().any(|other| other.name == field.name) {
            found.add(CODE, format!("{at}/name"));
        }
        found.text(&field.label, 1, 80, CODE, format!("{at}/label"));
        match &field.input {
            FormInput::Text { value: Some(v), .. } => {
                found.text(v, 0, 16_384, CODE, format!("{at}/value"))
            }
            FormInput::Choice { options, .. } => {
                found.count(options.len(), 2, 20, CODE, format!("{at}/options"))
            }
            FormInput::Document { value: Some(item) } => {
                refs.content(found, item, &format!("{at}/value"))
            }
            _ => {}
        }
    }
    let verb = f.verb.trim().to_lowercase();
    let forbidden = FORBIDDEN_VERBS.contains(&verb.as_str())
        || rc
            .approval_verbs
            .iter()
            .any(|v| v.trim().to_lowercase() == verb);
    if forbidden {
        found.add(CODE, "/verb");
    }
}

type Check<'a> = Pin<Box<dyn Future<Output = Option<BlockRejection>> + Send + 'a>>;

/// Resolves every collected reference, at most 16 at once, each within 2 s (10.4).
async fn resolve(
    refs: Refs,
    workspace_id: WorkspaceId,
    resolver: &dyn RefResolver,
) -> Vec<BlockRejection> {
    let checks: Vec<Check<'_>> = refs
        .items
        .into_iter()
        .map(|(pending, at, code)| {
            let check: Check<'_> = Box::pin(async move {
                let lookup = async {
                    match &pending {
                        Pending::Content(item) => resolver.content(workspace_id, item).await,
                        Pending::Email(link, id) => {
                            resolver.email_message(workspace_id, *link, id).await
                        }
                        Pending::Entity(entity) => resolver.entity(workspace_id, *entity).await,
                    }
                };
                let found = tokio::time::timeout(RESOLVE_TIMEOUT, lookup)
                    .await
                    .unwrap_or(false);
                (!found).then(|| rejection(code, at))
            });
            check
        })
        .collect();
    futures::stream::iter(checks)
        .buffered(RESOLVE_CONCURRENCY)
        .filter_map(|r| async move { r })
        .collect()
        .await
}

/// The rule context for `ctx`, asking the resolver for the producing run's agent once.
async fn rule_context<'a>(ctx: &BlockCheckContext, refs: &'a dyn RefResolver) -> RuleContext<'a> {
    let agent_name = match ctx.run_id {
        Some(run_id) => refs.run_agent_name(run_id).await,
        None => None,
    };
    RuleContext {
        now: ctx.now,
        agent_name,
        run_known: ctx.run_id.is_some(),
        approval_verbs: refs.approval_verbs(),
    }
}

/// Size and normalisation (10.1, 10.3); returns the normalised copy.
fn prepare(raw: &Value) -> Result<Value, Vec<BlockRejection>> {
    let size = serde_json::to_vec(raw)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    if size > MAX_BLOCK_BYTES {
        return Err(vec![rejection("BLOCK_TOO_LARGE", "")]);
    }
    let mut value = raw.clone();
    normalise(&mut value);
    Ok(value)
}

/// Logs `block.rejected` (counts and codes only, never content).
fn log_rejected(ctx_run: Option<RunId>, kind: &str, rejections: &[BlockRejection]) {
    let codes: Vec<&str> = rejections.iter().map(|r| r.code.as_str()).collect();
    let by = ctx_run.map_or_else(|| "ordin".to_owned(), |r| r.to_string());
    tracing::info!(
        run_id = by.as_str(),
        kind,
        codes = codes.join(",").as_str(),
        count = rejections.len(),
        "block.rejected"
    );
}

/// Merges rule and reference rejections, keeping at most 20.
fn merged(mut found: Vec<BlockRejection>, resolved: Vec<BlockRejection>) -> Vec<BlockRejection> {
    found.extend(resolved);
    found.truncate(MAX_REJECTIONS);
    found
}

/// The `show` tool: one block of any kind (Internal Logic 10).
pub async fn check_block(
    raw: &Value,
    ctx: &BlockCheckContext,
    refs: &dyn RefResolver,
) -> Result<Block, Vec<BlockRejection>> {
    let kind = raw
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let outcome = check_block_inner(raw, ctx, refs).await;
    if let Err(rejections) = &outcome {
        log_rejected(ctx.run_id, &kind, rejections);
    }
    outcome
}

async fn check_block_inner(
    raw: &Value,
    ctx: &BlockCheckContext,
    refs: &dyn RefResolver,
) -> Result<Block, Vec<BlockRejection>> {
    let value = prepare(raw)?;
    let block = parse_block(&value).map_err(|r| vec![r])?;
    let rc = rule_context(ctx, refs).await;
    let mut found = Found::default();
    let mut pending = Refs { items: Vec::new() };
    check_rules(&mut found, &mut pending, &block, &rc);
    let resolved = resolve(pending, ctx.workspace_id, refs).await;
    let all = merged(found.0, resolved);
    if all.is_empty() {
        Ok(block)
    } else {
        Err(all)
    }
}

/// The `report` tool (Internal Logic 11): a report keeps the parts that pass and lists the others.
pub async fn check_report_tool(
    raw: &Value,
    ctx: &BlockCheckContext,
    refs: &dyn RefResolver,
) -> Result<CheckedReport, Vec<BlockRejection>> {
    let kind = raw
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let outcome = check_report_inner(raw, ctx, refs).await;
    match &outcome {
        Err(rejections) => log_rejected(ctx.run_id, &kind, rejections),
        Ok(report) if !report.dropped_parts.is_empty() => {
            log_rejected(ctx.run_id, &kind, &report.dropped_parts)
        }
        Ok(_) => {}
    }
    outcome
}

async fn check_report_inner(
    raw: &Value,
    ctx: &BlockCheckContext,
    refs: &dyn RefResolver,
) -> Result<CheckedReport, Vec<BlockRejection>> {
    let value = prepare(raw)?;
    match value.get("kind").and_then(Value::as_str) {
        Some("table" | "chart" | "key_figures") => {
            let block = check_block_inner(&value, ctx, refs).await?;
            let args = match block {
                Block::Table(t) => ReportToolArgs::Table(t),
                Block::Chart(c) => ReportToolArgs::Chart(c),
                Block::KeyFigures(k) => ReportToolArgs::KeyFigures(k),
                // Only the three kinds matched above reach here.
                _ => return Err(vec![rejection("BLOCK_KIND_UNKNOWN", "/kind")]),
            };
            Ok(CheckedReport {
                block: args,
                dropped_parts: Vec::new(),
            })
        }
        Some("report") => check_report_parts(&value, ctx, refs).await,
        _ => Err(vec![rejection("BLOCK_KIND_UNKNOWN", "/kind")]),
    }
}

async fn check_report_parts(
    value: &Value,
    ctx: &BlockCheckContext,
    refs: &dyn RefResolver,
) -> Result<CheckedReport, Vec<BlockRejection>> {
    let envelope: ReportEnvelope =
        parse(&without_key(value, "kind"), "", value).map_err(|r| vec![r])?;
    let rc = rule_context(ctx, refs).await;
    let shell = ReportBlock {
        title: envelope.title.clone(),
        as_of: envelope.as_of,
        parts: Vec::new(),
    };
    let mut found = Found::default();
    check_report_envelope(&mut found, &shell, &rc);
    // The part count is the report's own rule, read from the parts as sent.
    found.0.retain(|r| r.code != "BLOCK_REPORT_PARTS_COUNT");
    found.count(
        envelope.parts.len(),
        1,
        12,
        "BLOCK_REPORT_PARTS_COUNT",
        "/parts",
    );
    if !found.0.is_empty() {
        return Err(found.0);
    }
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for (i, raw_part) in envelope.parts.iter().enumerate() {
        let base = format!("/parts/{i}");
        let part = match parse_part(raw_part, &base, value) {
            Ok(part) => part,
            Err(r) => {
                dropped.push(r);
                continue;
            }
        };
        let mut part_found = Found::default();
        let mut pending = Refs { items: Vec::new() };
        check_part(&mut part_found, &mut pending, &part, &base, &rc);
        let resolved = resolve(pending, ctx.workspace_id, refs).await;
        let rejections = merged(part_found.0, resolved);
        if rejections.is_empty() {
            kept.push(part);
        } else {
            dropped.extend(rejections);
        }
    }
    if kept.is_empty() {
        dropped.truncate(MAX_REJECTIONS);
        return Err(dropped);
    }
    dropped.truncate(MAX_REJECTIONS);
    Ok(CheckedReport {
        block: ReportToolArgs::Report(ReportBlock {
            title: envelope.title,
            as_of: envelope.as_of,
            parts: kept,
        }),
        dropped_parts: dropped,
    })
}

/// The `plan` tool (Internal Logic 12).
pub async fn check_plan_tool(
    raw: &Value,
    run_id: RunId,
    refs: &dyn RefResolver,
) -> Result<PlanToolArgs, Vec<BlockRejection>> {
    let outcome = check_plan_inner(raw, run_id, refs).await;
    if let Err(rejections) = &outcome {
        log_rejected(Some(run_id), "plan", rejections);
    }
    outcome
}

async fn check_plan_inner(
    raw: &Value,
    run_id: RunId,
    refs: &dyn RefResolver,
) -> Result<PlanToolArgs, Vec<BlockRejection>> {
    let value = prepare(raw)?;
    let args: PlanToolArgs = parse(&value, "", &value).map_err(|r| vec![r])?;
    let mut found = Found::default();
    match &args {
        PlanToolArgs::State { steps } => {
            let ok = (3..=7).contains(&steps.len())
                && steps.iter().all(|s| (1..=80).contains(&s.chars().count()));
            if !ok {
                found.add("BLOCK_PLAN_INVALID", "/steps");
            } else if refs.plan_state(run_id).await.versions >= 20 {
                found.add("BLOCK_PLAN_TOO_MANY_VERSIONS", "/steps");
            }
        }
        PlanToolArgs::Mark { position, .. } => {
            let state = refs.plan_state(run_id).await;
            if *position < 1 || *position > state.current_len {
                found.add("BLOCK_PLAN_POSITION_INVALID", "/position");
            }
        }
    }
    if found.0.is_empty() {
        Ok(args)
    } else {
        Err(found.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert!(is_date("2024-02-29"));
        assert!(!is_date("2023-02-29"));
        assert!(!is_date("2023-13-01"));
        assert!(!is_date("2023-04-31"));
        assert!(!is_date("2023-4-01"));
        assert!(!is_date("2023/04/01"));
        assert!(!is_date("abcd-ef-gh"));
        assert!(is_date("2000-02-29"));
        assert!(!is_date("1900-02-29"));
    }

    #[test]
    fn reasons_are_one_short_sentence() {
        for code in crate::error::ALL_CODES
            .iter()
            .filter(|c| c.starts_with("BLOCK_"))
        {
            let r = reason(code);
            assert!((1..=300).contains(&r.chars().count()), "{code}");
        }
        assert!(!reason("OTHER").is_empty());
    }

    #[test]
    fn quoted_and_pointer_helpers() {
        assert_eq!(quoted("unknown field `force`, expected x"), Some("force"));
        assert_eq!(quoted("no ticks"), None);
        assert_eq!(
            kind_of(&serde_json::json!({"kind": "nope"}))
                .unwrap_err()
                .at,
            "/kind"
        );
        assert_eq!(without_key(&Value::Null, "kind"), Value::Null);
        let mut v = serde_json::json!(["e\u{301}", {"a": "\u{1}"}]);
        normalise(&mut v);
        assert_eq!(v[0], "é");
        assert_eq!(v[1]["a"], "\u{1}");
    }
}
