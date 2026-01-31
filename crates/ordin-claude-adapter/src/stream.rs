//! The CLI's stream-json output, one line at a time, parsed into typed items (ClaudeLocalAdapter.md
//! *Reading the CLI's output*; InputValidation: harness output parsed into typed events). Tool calls
//! in the stream are never used for enforcement: that is the hook's job (09 ADR-045).

use serde_json::Value;

/// Longest stdout line kept, 4 MiB.
pub(crate) const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
/// `rate_limit_info.status` of a used-up allowance. Read from the 2.1.316 program, whose
/// `rate_limit_event` carries `rate_limit_info: { status: allowed | allowed_warning | rejected }`.
const ALLOWANCE_USED_UP: &str = "rejected";

/// The fields of one `result` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnResult {
    pub is_error: bool,
    pub result: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StreamItem {
    /// `system` with subtype `init`.
    Init,
    /// `assistant` or `user`; `plan` is the harness's to-do list when the line carries one (C-41).
    Activity {
        plan: Option<Vec<String>>,
    },
    RateLimit {
        allowance_used_up: bool,
    },
    Result(TurnResult),
    Other,
}

/// Not JSON.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Malformed;

/// C-41: the `TodoWrite` tool input of an `assistant` line, each step's text.
// spec: ordin-desktop/components/ClaudeLocalAdapter#C-41
fn plan_of(line: &Value) -> Option<Vec<String>> {
    let blocks = line.pointer("/message/content")?.as_array()?;
    let input = blocks.iter().find_map(|block| {
        (block.get("type")?.as_str()? == "tool_use" && block.get("name")?.as_str()? == "TodoWrite")
            .then(|| block.get("input"))
            .flatten()
    })?;
    let todos = input.get("todos")?.as_array()?;
    todos
        .iter()
        .map(|todo| todo.get("content")?.as_str().map(str::to_owned))
        .collect()
}

fn turn_result(line: &Value) -> TurnResult {
    // C-37: where usage is absent the token fields stay `None`.
    let usage = |key: &str| line.get("usage")?.get(key)?.as_u64();
    TurnResult {
        is_error: line.get("is_error").and_then(Value::as_bool) == Some(true),
        result: line
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_owned),
        input_tokens: usage("input_tokens"),
        output_tokens: usage("output_tokens"),
        cached_input_tokens: usage("cache_read_input_tokens"),
    }
}

pub(crate) fn parse_line(bytes: &[u8]) -> Result<StreamItem, Malformed> {
    let line: Value = serde_json::from_slice(bytes).map_err(|_| Malformed)?;
    let kind = line.get("type").and_then(Value::as_str);
    Ok(match kind {
        Some("system") if line.get("subtype").and_then(Value::as_str) == Some("init") => {
            StreamItem::Init
        }
        Some("assistant") => StreamItem::Activity {
            plan: plan_of(&line),
        },
        Some("user") => StreamItem::Activity { plan: None },
        Some("rate_limit_event") => StreamItem::RateLimit {
            allowance_used_up: line
                .pointer("/rate_limit_info/status")
                .and_then(Value::as_str)
                == Some(ALLOWANCE_USED_UP),
        },
        Some("result") => StreamItem::Result(turn_result(&line)),
        _ => StreamItem::Other,
    })
}
