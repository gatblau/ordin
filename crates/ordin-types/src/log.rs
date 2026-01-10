//! The one log line form of the workspace (Logging.md, module `log`): the formatter, the field rules
//! and the argument digest. Installed by the two composition roots; every crate writes lines through
//! the `tracing` macros only.
//!
//! This module is the one place the crate touches I/O or global state: SharedTypes.md describes the
//! crate as pure, and Logging.md places the formatter and `install` here. Nothing else in the crate
//! logs, and nothing here runs unless a program calls `install`.

use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::encoding::canonical_json;
use crate::num::Sha256Hex;
use crate::time::Timestamp;

pub const EVENT_NAME: &str = r"^[a-z][a-z0-9_.]{0,63}$"; // wrightrun Logging spec
pub const FIELD_NAME: &str = r"^[a-z][a-z0-9_]{0,39}$"; // wrightrun Logging spec
pub const RESERVED_KEYS: [&str; 7] = [
    "time",
    "level",
    "logger",
    "event",
    "run_id",
    "crew_run_id",
    "thread_id",
];
pub const FORBIDDEN_FIELDS: [&str; 14] = [
    "token",
    "secret",
    "password",
    "authorization",
    "signature",
    "cookie",
    "url",
    "uri",
    "dsn",
    "address",
    "content",
    "body",
    "arguments",
    "prompt",
]; // wrightrun Logging spec
pub const MAX_FIELD_CHARS: usize = 4_000; // wrightrun Logging spec
pub const MAX_REPORT_CHARS: usize = 262_144; // field `report` of event `startup_report` only
pub const MAX_LIST_ITEMS: usize = 200; // wrightrun Logging spec
pub const MAX_PANIC_MESSAGE_CHARS: usize = 200; // Ordin Desktop spec, step 2

/// The four identifiers a line takes from its enclosing spans, in line order (R-1).
const BINDINGS: [&str; 4] = ["run_id", "crew_run_id", "thread_id", "correlation_id"];
/// What a cut value ends with (R-4).
const CUT_MARK: &str = "…[cut]";
/// The length a library's message is cut to (R-5).
const MAX_TEMPLATE_CHARS: usize = 200;

/// The one line formatter, used by apps/desktop and apps/server.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonLineFormat;

/// Where lines go, chosen by the program.
#[derive(Debug, Clone)]
pub enum LogSink {
    /// Daily files `<dir>/<prefix>.<YYYY-MM-DD>.<suffix>`, `keep` files kept.
    DailyFiles {
        dir: std::path::PathBuf,
        prefix: &'static str,
        suffix: &'static str,
        keep: usize,
    },
    Stdout,
    /// Server commands that print a result (every command but `start`, `agent start`, `agent job run-one`).
    Stderr,
}

/// `ORDIN_LOG_LEVEL` held a value other than `error`, `warn`, `info` or `debug`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogLevelInvalid; // mapped by the server program to SERVER_ENV_INVALID

/// Parses ORDIN_LOG_LEVEL; `None` (unset or empty) → Info.
pub fn level_from_env(raw: Option<&str>) -> Result<Level, LogLevelInvalid> {
    match raw {
        None | Some("") => Ok(Level::INFO),
        Some("error") => Ok(Level::ERROR),
        Some("warn") => Ok(Level::WARN),
        Some("info") => Ok(Level::INFO),
        Some("debug") => Ok(Level::DEBUG),
        Some(_) => Err(LogLevelInvalid),
    }
}

/// Installs the global subscriber once: JsonLineFormat, the sink, the level filter
/// (ordin crates at `level`; every other crate at WARN). Returns the appender guard the program holds.
///
/// `None` when the daily-file appender cannot be built or a global subscriber is already installed;
/// in both cases no line is written through this call.
pub fn install(sink: LogSink, level: Level) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt as _;

    let (writer, guard) = match sink {
        LogSink::DailyFiles {
            dir,
            prefix,
            suffix,
            keep,
        } => {
            let appender = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix(prefix)
                .filename_suffix(suffix)
                .max_log_files(keep)
                .build(dir)
                .ok()?;
            tracing_appender::non_blocking(appender)
        }
        LogSink::Stdout => tracing_appender::non_blocking(std::io::stdout()),
        LogSink::Stderr => tracing_appender::non_blocking(std::io::stderr()),
    };
    let lines = tracing_subscriber::fmt::layer()
        .event_format(JsonLineFormat)
        .with_writer(writer)
        .with_filter(tracing_subscriber::filter::filter_fn(move |meta| {
            let max = if is_ordin_target(meta.target()) {
                level
            } else {
                Level::WARN
            };
            // More verbose levels compare greater (TRACE > DEBUG > … > ERROR).
            *meta.level() <= max
        }));
    let subscriber = tracing_subscriber::registry()
        .with(BindingLayer)
        .with(lines);
    tracing::subscriber::set_global_default(subscriber).ok()?;
    Some(guard)
}

/// SHA-256 of the canonical JSON of tool arguments (rule R-3). The only form in which
/// arguments reach a log line, a journal row or a Rules Engine record.
pub fn arguments_digest(arguments: &serde_json::Value) -> Sha256Hex {
    let digest: [u8; 32] = Sha256::digest(canonical_json(arguments)).into();
    Sha256Hex::from_digest(&digest)
}

/// The targets the level filter treats as ordin's (V-1): `ordin_*` crates, the desktop program
/// (`ordin`), and the embedded `memory` and `sandboxd`.
fn is_ordin_target(target: &str) -> bool {
    let root = target.split("::").next().unwrap_or(target);
    root == "ordin" || root.starts_with("ordin_") || root == "memory" || root == "sandboxd"
}

/// `^[a-z][a-z0-9_.]{0,63}$` (EVENT_NAME).
fn is_event_name(name: &str) -> bool {
    is_name(name, 64, |b| b == b'_' || b == b'.')
}

/// `^[a-z][a-z0-9_]{0,39}$` (FIELD_NAME).
fn is_field_name(name: &str) -> bool {
    is_name(name, 40, |b| b == b'_')
}

fn is_name(name: &str, max: usize, extra: fn(u8) -> bool) -> bool {
    let bytes = name.as_bytes();
    match bytes.split_first() {
        Some((first, rest)) => {
            first.is_ascii_lowercase()
                && bytes.len() <= max
                && rest
                    .iter()
                    .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || extra(b))
        }
        None => false,
    }
}

/// R-2: a field written under its own name, or dropped and listed in `dropped_fields`.
fn is_field_allowed(name: &str) -> bool {
    if !is_field_name(name) {
        return false;
    }
    if RESERVED_KEYS.contains(&name) && !BINDINGS.contains(&name) {
        return false;
    }
    !FORBIDDEN_FIELDS.iter().any(|word| {
        name == *word
            || name
                .strip_suffix(word)
                .is_some_and(|stem| stem.ends_with('_'))
    })
}

/// R-4: a string longer than `max` characters is cut and marked.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}{CUT_MARK}", &text[..at]),
        None => text.to_owned(),
    }
}

/// The value of one recorded field, before the line's rules are applied.
enum Recorded {
    Value(Value),
    /// A value recorded through `Debug` (`?x`, and `%x`, which `tracing` records the same way):
    /// kept as text only where the line needs it (the event name and the binding identifiers).
    Debug(String),
}

/// Collects an event's fields in call order.
#[derive(Default)]
struct FieldCollector {
    message: Option<String>,
    fields: Vec<(&'static str, Recorded)>,
}

impl Visit for FieldCollector {
    fn record_f64(&mut self, field: &Field, value: f64) {
        // A non-finite float becomes `null` (R-4).
        let value = serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number);
        self.fields.push((field.name(), Recorded::Value(value)));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .push((field.name(), Recorded::Value(Value::from(value))));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .push((field.name(), Recorded::Value(Value::from(value))));
    }
    fn record_i128(&mut self, field: &Field, value: i128) {
        let value =
            i64::try_from(value).map_or_else(|_| Value::from(value.to_string()), Value::from);
        self.fields.push((field.name(), Recorded::Value(value)));
    }
    fn record_u128(&mut self, field: &Field, value: u128) {
        let value =
            u64::try_from(value).map_or_else(|_| Value::from(value.to_string()), Value::from);
        self.fields.push((field.name(), Recorded::Value(value)));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .push((field.name(), Recorded::Value(Value::Bool(value))));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_owned());
        } else {
            self.fields
                .push((field.name(), Recorded::Value(Value::from(value))));
        }
    }
    fn record_error(&mut self, field: &Field, _value: &(dyn std::error::Error + 'static)) {
        // R-6: an error is logged as its `code()` by the caller, never as its text.
        self.fields
            .push((field.name(), Recorded::Debug(String::new())));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let mut text = String::new();
        let _ = write!(text, "{value:?}");
        if field.name() == "message" {
            self.message = Some(text);
        } else {
            self.fields.push((field.name(), Recorded::Debug(text)));
        }
    }
}

/// The binding identifiers a span recorded, kept in the span's extensions by [`BindingLayer`].
#[derive(Default)]
struct Bindings([Option<String>; 4]);

impl Visit for Bindings {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.set(field.name(), value.to_owned());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let mut text = String::new();
        let _ = write!(text, "{value:?}");
        self.set(field.name(), text);
    }
}

impl Bindings {
    fn set(&mut self, name: &str, value: String) {
        if let Some(at) = BINDINGS.iter().position(|b| *b == name) {
            self.0[at] = Some(value);
        }
    }
}

/// Records the binding identifiers of every span, so the formatter can read them (L-3).
struct BindingLayer;

impl<S> Layer<S> for BindingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut bindings = Bindings::default();
        attrs.record(&mut bindings);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(bindings);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            match extensions.get_mut::<Bindings>() {
                Some(bindings) => values.record(bindings),
                None => {
                    let mut bindings = Bindings::default();
                    values.record(&mut bindings);
                    extensions.insert(bindings);
                }
            }
        }
    }
}

/// Appends `"key":value` to the line.
fn push_pair(line: &mut String, key: &str, value: &Value) {
    if line.len() > 1 {
        line.push(',');
    }
    // Writing a `Value` to text cannot fail; a failure would leave the pair's value out.
    let _ = write!(
        line,
        "{}:{}",
        Value::from(key),
        serde_json::to_string(value).unwrap_or_default()
    );
}

fn level_word(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARNING",
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
}

impl<S, N> FormatEvent<S, N> for JsonLineFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        let mut collected = FieldCollector::default();
        event.record(&mut collected);
        let library = !is_ordin_target(meta.target());

        let mut line = String::from("{");
        push_pair(&mut line, "time", &Value::from(Timestamp::now().to_wire()));
        push_pair(&mut line, "level", &Value::from(level_word(meta.level())));
        push_pair(&mut line, "logger", &Value::from(meta.target()));

        // The event's name is its message; a library's line is `library_record` (R-2, R-5).
        let message = collected.message.take().unwrap_or_default();
        let mut tail: Vec<(String, Value)> = Vec::new();
        let event_name = if library {
            tail.push((
                "template".to_owned(),
                Value::from(cut(&message, MAX_TEMPLATE_CHARS)),
            ));
            "library_record".to_owned()
        } else if is_event_name(&message) {
            message
        } else {
            tail.push((
                "name_chars".to_owned(),
                Value::from(message.chars().count()),
            ));
            "invalid_event_name".to_owned()
        };
        let report_event = event_name == "startup_report";
        push_pair(&mut line, "event", &Value::from(event_name));

        // The binding identifiers: the innermost span that recorded each, else the event's field.
        let mut bound: [Option<String>; 4] = Default::default();
        if let Some(scope) = ctx.event_scope() {
            for span in scope {
                if let Some(bindings) = span.extensions().get::<Bindings>() {
                    for (slot, value) in bound.iter_mut().zip(&bindings.0) {
                        if slot.is_none() {
                            slot.clone_from(value);
                        }
                    }
                }
            }
        }
        let mut conflict = false;
        let mut dropped: Vec<Value> = Vec::new();
        let mut fields: Vec<(&'static str, Value)> = Vec::new();
        if !library {
            for (name, recorded) in collected.fields {
                if let Some(at) = BINDINGS.iter().position(|b| *b == name) {
                    // A binding identifier given on the event is written as given (L-3).
                    let given = match recorded {
                        Recorded::Value(Value::String(text)) | Recorded::Debug(text) => text,
                        Recorded::Value(other) => other.to_string(),
                    };
                    if bound[at].as_ref().is_some_and(|b| *b != given) {
                        conflict = true;
                    }
                    bound[at] = Some(given);
                    continue;
                }
                if !is_field_allowed(name) {
                    if dropped.len() < MAX_LIST_ITEMS {
                        dropped.push(Value::from(name));
                    }
                    continue;
                }
                let max = if report_event && name == "report" {
                    MAX_REPORT_CHARS
                } else {
                    MAX_FIELD_CHARS
                };
                let value = match recorded {
                    Recorded::Value(Value::String(text)) => Value::from(cut(&text, max)),
                    Recorded::Value(other) => other,
                    // R-4: a `Debug` value may hold content, so it never reaches a line.
                    Recorded::Debug(_) => Value::from("<debug>"),
                };
                fields.push((name, value));
            }
        }
        for (name, value) in BINDINGS.iter().zip(bound) {
            if let Some(value) = value {
                push_pair(&mut line, name, &Value::from(cut(&value, MAX_FIELD_CHARS)));
            }
        }
        for (name, value) in &tail {
            push_pair(&mut line, name, value);
        }
        for (name, value) in &fields {
            push_pair(&mut line, name, value);
        }
        if conflict {
            push_pair(&mut line, "binding_conflict", &Value::Bool(true));
        }
        if !dropped.is_empty() {
            push_pair(&mut line, "dropped_fields", &Value::Array(dropped));
        }
        line.push('}');
        writeln!(writer, "{line}")
    }
}

#[cfg(test)]
mod tests {
    //! The formatter-level scenarios of Logging.md, over an in-memory writer.

    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt as _;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Runs `body` with the formatter installed for this thread and returns the lines written.
    fn lines(body: impl FnOnce()) -> Vec<String> {
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::registry().with(BindingLayer).with(
            tracing_subscriber::fmt::layer()
                .event_format(JsonLineFormat)
                .with_writer(move || writer.clone()),
        );
        tracing::subscriber::with_default(subscriber, body);
        let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        text.lines().map(str::to_owned).collect()
    }

    fn keys(line: &str) -> Vec<String> {
        // `serde_json` without `preserve_order` sorts keys, so the order is read from the text.
        let value: Value = serde_json::from_str(line).unwrap();
        let mut found: Vec<(usize, String)> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|k| (line.find(&format!("\"{k}\":")).unwrap(), k.clone()))
            .collect();
        found.sort();
        found.into_iter().map(|(_, k)| k).collect()
    }

    // Scenario: Happy path — a tool call line carries the run and the digest only
    #[test]
    fn happy_path_a_tool_call_line_carries_the_run_and_the_digest_only() {
        let arguments = serde_json::json!({"query": "Q3 invoices"});
        let digest = arguments_digest(&arguments);
        let written = lines(|| {
            let crew =
                tracing::info_span!("crew", crew_run_id = "0192a1c2-0000-7a10-8b20-0c3d4e5f6a7b");
            let _crew = crew.enter();
            let run = tracing::info_span!(
                "ordin.run_stretch",
                run_id = "0192a1c2-3344-7556-8677-8899aabbccdd"
            );
            let _run = run.enter();
            let call = tracing::info_span!(
                "call",
                correlation_id = "0192a1c2-3344-7556-8677-8899aabbccdd:1"
            );
            let _call = call.enter();
            tracing::info!(
                target: "ordin_mcp_front",
                tool = "search",
                arguments_digest = digest.0.as_str(),
                "mcp_front.call"
            );
        });
        assert_eq!(written.len(), 1);
        assert_eq!(
            keys(&written[0]),
            [
                "time",
                "level",
                "logger",
                "event",
                "run_id",
                "crew_run_id",
                "correlation_id",
                "tool",
                "arguments_digest"
            ]
        );
        let line: Value = serde_json::from_str(&written[0]).unwrap();
        assert_eq!(line["logger"], "ordin_mcp_front");
        assert_eq!(line["level"], "INFO");
        assert_eq!(line["event"], "mcp_front.call");
        let expected: [u8; 32] = Sha256::digest(br#"{"query":"Q3 invoices"}"#).into();
        assert_eq!(
            line["arguments_digest"],
            Sha256Hex::from_digest(&expected).0
        );
        assert!(!written[0].contains("Q3 invoices"));
        assert!(crate::time::parse_rfc3339_ms_utc(line["time"].as_str().unwrap()).is_ok());
    }

    // Scenario: Edge case — a forbidden field name is dropped, not written
    #[test]
    fn edge_case_a_forbidden_field_name_is_dropped_not_written() {
        let written = lines(|| {
            tracing::info!(
                connection = "tickets",
                bot_token = "xoxb-1",
                "connection_bound"
            );
        });
        let line: Value = serde_json::from_str(&written[0]).unwrap();
        assert_eq!(line["connection"], "tickets");
        assert_eq!(line["dropped_fields"], serde_json::json!(["bot_token"]));
        assert!(!written[0].contains("xoxb-1"));
    }

    // Scenario: Edge case — a long value is cut
    #[test]
    fn edge_case_a_long_value_is_cut() {
        let reason = "r".repeat(5_000);
        let written = lines(|| tracing::warn!(reason = reason.as_str(), "refused"));
        let line: Value = serde_json::from_str(&written[0]).unwrap();
        assert_eq!(line["reason"], format!("{}{CUT_MARK}", "r".repeat(4_000)));
        assert_eq!(line["level"], "WARNING");
    }

    // Scenario: Error path — an unknown log level stops the server (the parse half; the exit is the server's)
    #[test]
    fn error_path_an_unknown_log_level_stops_the_server() {
        assert_eq!(level_from_env(Some("verbose")), Err(LogLevelInvalid));
        assert_eq!(level_from_env(None), Ok(Level::INFO));
        assert_eq!(level_from_env(Some("")), Ok(Level::INFO));
        assert_eq!(level_from_env(Some("error")), Ok(Level::ERROR));
        assert_eq!(level_from_env(Some("warn")), Ok(Level::WARN));
        assert_eq!(level_from_env(Some("info")), Ok(Level::INFO));
        assert_eq!(level_from_env(Some("debug")), Ok(Level::DEBUG));
    }

    #[test]
    fn values_follow_rule_r4_and_names_rule_r2() {
        let written = lines(|| {
            let detail = vec![1, 2];
            tracing::error!(
                ratio = f64::NAN,
                count = 3u64,
                delta = -4i64,
                large = u128::MAX,
                small = -5i128,
                ok = true,
                detail = ?detail,
                gateway_url = "https://x",
                Level = 1,
                time = "now",
                "Not An Event"
            );
        });
        let line: Value = serde_json::from_str(&written[0]).unwrap();
        assert_eq!(line["event"], "invalid_event_name");
        assert_eq!(line["name_chars"], 12);
        assert_eq!(line["ratio"], Value::Null);
        assert_eq!(line["count"], 3);
        assert_eq!(line["delta"], -4);
        assert_eq!(line["large"], u128::MAX.to_string());
        assert_eq!(line["small"], -5);
        assert_eq!(line["ok"], true);
        assert_eq!(line["detail"], "<debug>");
        assert_eq!(line["level"], "ERROR");
        assert_eq!(
            line["dropped_fields"],
            serde_json::json!(["gateway_url", "Level", "time"])
        );
        assert!(!written[0].contains("https://x"));
    }

    #[test]
    fn a_binding_given_on_the_event_wins_and_is_marked() {
        let written = lines(|| {
            let span = tracing::info_span!("run", run_id = tracing::field::Empty);
            span.record("run_id", "0192a1c2-3344-7556-8677-8899aabbccdd");
            let _entered = span.enter();
            tracing::info!(
                run_id = "0192a1c2-3344-7556-8677-000000000000",
                thread_id = 7,
                "run_summary"
            );
            tracing::info!("run_summary");
        });
        let first: Value = serde_json::from_str(&written[0]).unwrap();
        assert_eq!(first["run_id"], "0192a1c2-3344-7556-8677-000000000000");
        assert_eq!(first["thread_id"], "7");
        assert_eq!(first["binding_conflict"], true);
        let second: Value = serde_json::from_str(&written[1]).unwrap();
        assert_eq!(second["run_id"], "0192a1c2-3344-7556-8677-8899aabbccdd");
        assert!(second.get("binding_conflict").is_none());
    }

    #[test]
    fn a_library_line_keeps_only_its_template() {
        let message = "m".repeat(300);
        let written = lines(|| {
            tracing::warn!(target: "hyper::client", address = "10.0.0.1", "{}", message);
        });
        let line: Value = serde_json::from_str(&written[0]).unwrap();
        assert_eq!(line["event"], "library_record");
        assert_eq!(line["template"], format!("{}{CUT_MARK}", "m".repeat(200)));
        assert!(line.get("address").is_none());
    }

    #[test]
    fn the_startup_report_field_takes_the_larger_limit() {
        let report = "x".repeat(5_000);
        let written = lines(|| {
            tracing::info!(report = report.as_str(), "startup_report");
            tracing::info!(report = report.as_str(), "other_event");
        });
        let first: Value = serde_json::from_str(&written[0]).unwrap();
        assert_eq!(first["report"].as_str().unwrap().len(), 5_000);
        let second: Value = serde_json::from_str(&written[1]).unwrap();
        assert!(second["report"].as_str().unwrap().ends_with(CUT_MARK));
    }

    #[test]
    fn ordin_targets_and_names() {
        assert!(is_ordin_target("ordin"));
        assert!(is_ordin_target("ordin_mcp_front::call"));
        assert!(is_ordin_target("memory::service"));
        assert!(is_ordin_target("sandboxd"));
        assert!(!is_ordin_target("ordinal"));
        assert!(!is_ordin_target("hyper"));
        assert!(is_event_name("mcp_front.call"));
        assert!(!is_event_name(&"a".repeat(65)));
        assert!(!is_event_name(""));
        assert!(is_field_name(&"a".repeat(40)));
        assert!(!is_field_name(&"a".repeat(41)));
        assert!(!is_field_allowed("password"));
        assert!(!is_field_allowed("db_dsn"));
        assert!(is_field_allowed("tokens"));
        assert!(is_field_allowed("run_id"));
    }

    #[test]
    fn install_writes_daily_files_once() {
        let dir = std::env::temp_dir().join(format!("ordin-types-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sink = LogSink::DailyFiles {
            dir: dir.clone(),
            prefix: "ordin",
            suffix: "log",
            keep: 14,
        };
        let guard = install(sink, Level::INFO).expect("the first install");
        // The global subscriber is set; a second install is refused.
        assert!(install(LogSink::Stderr, Level::INFO).is_none());
        tracing::info!(target: "ordin_test", "install_check");
        drop(guard);
        let written: String = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect();
        assert!(written.contains("\"event\":\"install_check\""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
