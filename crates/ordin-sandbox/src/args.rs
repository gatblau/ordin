//! The tools' arguments: the fixed-code check (`check_arguments`) and the typed value
//! (Sandbox.md *Internal Logic* B.3 and C.2).

use std::collections::BTreeMap;

use ordin_mcp_front::ArgumentProblem;
use serde::Deserialize;
use serde_json::{json, Value};

/// The longest `code` accepted: 256 KiB.
pub(crate) const MAX_CODE_BYTES: usize = 262_144;
const MAX_CAPABILITIES: usize = 50;
const MAX_FILES: usize = 1_000;
const MAX_INPUTS: usize = 100;
/// The capability name reserved for the `ordin_tools` module.
pub(crate) const RESERVED_CAPABILITY: &str = "ordin_tools";

/// The typed arguments of `python_sandbox` (and the two further tools under `extra-guests`).
// spec: ordin-desktop/components/Sandbox#SandboxArgs
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SandboxArgs {
    pub code: String,
    #[serde(default)]
    pub requested_capabilities: Vec<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub files: Vec<FileArg>,
    #[serde(default)]
    pub inputs: Vec<InputArg>,
    #[serde(default)]
    pub work_session: bool,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FileArg {
    pub path: String,
    pub content_base64: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InputArg {
    pub place_id: String,
    pub path: String,
    #[serde(default, rename = "as")]
    pub as_path: Option<String>,
}

/// The JSON Schema of the three tools (Sandbox.md *MCP tools*).
pub(crate) fn input_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["code"],
        "properties": {
            "code":                  { "type": "string", "minLength": 1, "maxLength": 262144 },
            "requestedCapabilities": { "type": "array", "items": { "type": "string" }, "maxItems": 50 },
            "dryRun":                { "type": "boolean" },
            "files": {
                "type": "array", "maxItems": 1000,
                "items": { "type": "object", "additionalProperties": false, "required": ["path", "contentBase64"],
                           "properties": { "path": { "type": "string" }, "contentBase64": { "type": "string" } } }
            },
            "inputs": {
                "type": "array", "maxItems": 100,
                "items": { "type": "object", "additionalProperties": false, "required": ["placeId", "path"],
                           "properties": { "placeId": { "type": "string", "format": "uuid" },
                                           "path":    { "type": "string" },
                                           "as":      { "type": "string" } } }
            },
            "workSession":           { "type": "boolean" },
            "environment":           { "type": "object", "additionalProperties": { "type": "string" } }
        }
    })
}

fn problem(at: &str, field: &str, rule: &str) -> ArgumentProblem {
    let mut reason = format!("`{field}` is not valid: {rule}.");
    if reason.len() > 300 {
        reason.truncate(297);
        reason.push_str("...");
    }
    ArgumentProblem {
        at: at.to_owned(),
        reason,
    }
}

fn strings_object(
    item: &Value,
    at: &str,
    field: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<(), ArgumentProblem> {
    let Some(map) = item.as_object() else {
        return Err(problem(at, field, "an object is expected"));
    };
    for key in map.keys() {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            return Err(problem(
                &format!("{at}/{key}"),
                &format!("{field}.{key}"),
                "unknown field",
            ));
        }
    }
    for key in required {
        if !map.contains_key(*key) {
            return Err(problem(at, &format!("{field}.{key}"), "required"));
        }
    }
    for (key, value) in map {
        if !value.is_string() {
            return Err(problem(
                &format!("{at}/{key}"),
                &format!("{field}.{key}"),
                "a string is expected",
            ));
        }
    }
    Ok(())
}

fn array<'a>(value: &'a Value, field: &str, max: usize) -> Result<&'a Vec<Value>, ArgumentProblem> {
    let at = format!("/{field}");
    let Some(items) = value.as_array() else {
        return Err(problem(&at, field, "an array is expected"));
    };
    if items.len() > max {
        return Err(problem(&at, field, &format!("at most {max} entries")));
    }
    Ok(items)
}

/// The fixed-code argument check (B.3, C.2). `environment` without `extra-guests` passes here and
/// is refused by `call` (SBX_ENVIRONMENT_UNSUPPORTED).
// spec: ordin-desktop/components/Sandbox#SBX_ARGS_INVALID
pub(crate) fn check(arguments: &Value) -> Result<SandboxArgs, ArgumentProblem> {
    let Some(map) = arguments.as_object() else {
        return Err(problem("", "arguments", "an object is expected"));
    };
    const KNOWN: [&str; 7] = [
        "code",
        "requestedCapabilities",
        "dryRun",
        "files",
        "inputs",
        "workSession",
        "environment",
    ];
    for key in map.keys() {
        if !KNOWN.contains(&key.as_str()) {
            return Err(problem(&format!("/{key}"), key, "unknown field"));
        }
    }
    match map.get("code") {
        None => return Err(problem("/code", "code", "required")),
        Some(Value::String(code)) if code.is_empty() => {
            return Err(problem("/code", "code", "must not be empty"))
        }
        Some(Value::String(code)) if code.len() > MAX_CODE_BYTES => {
            return Err(problem("/code", "code", "at most 256 KiB"))
        }
        Some(Value::String(_)) => {}
        Some(_) => return Err(problem("/code", "code", "a string is expected")),
    }
    if let Some(value) = map.get("requestedCapabilities") {
        let items = array(value, "requestedCapabilities", MAX_CAPABILITIES)?;
        for (i, item) in items.iter().enumerate() {
            let at = format!("/requestedCapabilities/{i}");
            match item.as_str() {
                None => {
                    return Err(problem(
                        &at,
                        "requestedCapabilities",
                        "a string is expected",
                    ))
                }
                Some(RESERVED_CAPABILITY) => {
                    return Err(problem(
                        &at,
                        "requestedCapabilities",
                        "ordin_tools is reserved",
                    ))
                }
                Some(_) => {}
            }
        }
    }
    for flag in ["dryRun", "workSession"] {
        if let Some(value) = map.get(flag) {
            if !value.is_boolean() {
                return Err(problem(
                    &format!("/{flag}"),
                    flag,
                    "true or false is expected",
                ));
            }
        }
    }
    if let Some(value) = map.get("files") {
        for (i, item) in array(value, "files", MAX_FILES)?.iter().enumerate() {
            strings_object(
                item,
                &format!("/files/{i}"),
                "files",
                &["path", "contentBase64"],
                &[],
            )?;
        }
    }
    if let Some(value) = map.get("inputs") {
        for (i, item) in array(value, "inputs", MAX_INPUTS)?.iter().enumerate() {
            let at = format!("/inputs/{i}");
            strings_object(item, &at, "inputs", &["placeId", "path"], &["as"])?;
            let place = item["placeId"].as_str().unwrap_or_default();
            if ordin_types::PlaceId::parse(place).is_err() {
                return Err(problem(
                    &format!("{at}/placeId"),
                    "inputs.placeId",
                    "a place identifier is expected",
                ));
            }
        }
    }
    if let Some(value) = map.get("environment") {
        let Some(vars) = value.as_object() else {
            return Err(problem(
                "/environment",
                "environment",
                "an object is expected",
            ));
        };
        for (name, value) in vars {
            if !value.is_string() {
                return Err(problem(
                    &format!("/environment/{name}"),
                    "environment",
                    "values are strings",
                ));
            }
        }
        #[cfg(feature = "extra-guests")]
        for (name, value) in vars {
            environment::check(name, value.as_str().unwrap_or_default())?;
        }
    }
    serde_json::from_value(arguments.clone())
        .map_err(|_| problem("", "arguments", "they do not match the tool's schema"))
}

/// The run environment's rules (C.2), compiled only with the further guests.
#[cfg(feature = "extra-guests")]
mod environment {
    use ordin_mcp_front::ArgumentProblem;

    const RESERVED: [&str; 3] = ["PATH", "HOME", "PWD"];

    fn secret_shaped(value: &str) -> bool {
        // The controller's redaction pattern (controller.rs:350), and two token prefixes.
        let patterns = [
            r"eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+|Bearer\s+[A-Za-z0-9._\-]+",
            r"gh[pousr]_[A-Za-z0-9]{20,}",
            r"sk-[A-Za-z0-9]{20,}",
        ];
        patterns
            .iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .any(|re| re.is_match(value))
    }

    fn plain_name(name: &str) -> bool {
        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        (first.is_ascii_alphabetic() || first == '_')
            && name.len() <= 64
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    // spec: ordin-desktop/components/Sandbox#SBX_ENVIRONMENT_INVALID
    pub(super) fn check(name: &str, value: &str) -> Result<(), ArgumentProblem> {
        let refused = !plain_name(name)
            || RESERVED.contains(&name)
            || name.starts_with("WASI_")
            || value.len() > 4096
            || secret_shaped(value);
        if refused {
            tracing::info!(target: "ordin_sandbox", code = "SBX_ENVIRONMENT_INVALID", "sandbox.args_refused");
            return Err(ArgumentProblem {
                at: "/environment".to_owned(),
                reason: format!(
                    "Variable `{name}` is refused: names are plain identifiers and values may not hold secrets."
                ),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimal_call_is_accepted() {
        let args = check(&json!({"code": "print(1)"})).unwrap();
        assert_eq!(args.code, "print(1)");
        assert!(!args.work_session && !args.dry_run);
        assert!(args.files.is_empty() && args.inputs.is_empty());
    }

    #[test]
    fn every_field_is_read() {
        let place = ordin_types::PlaceId::new().to_string();
        let args = check(&json!({
            "code": "x", "requestedCapabilities": ["tickets"], "dryRun": true,
            "files": [{"path": "a.txt", "contentBase64": "eA=="}],
            "inputs": [{"placeId": place, "path": "Q3/rates.csv", "as": "r.csv"}],
            "workSession": true, "environment": {"MODE": "fast"}
        }))
        .unwrap();
        assert_eq!(args.requested_capabilities, ["tickets"]);
        assert_eq!(args.inputs[0].as_path.as_deref(), Some("r.csv"));
        assert_eq!(args.environment["MODE"], "fast");
    }

    #[test]
    fn refusals_name_the_field() {
        let place = ordin_types::PlaceId::new().to_string();
        let cases = [
            (json!([]), ""),
            (json!({}), "/code"),
            (json!({"code": ""}), "/code"),
            (json!({"code": 1}), "/code"),
            (json!({"code": "x".repeat(MAX_CODE_BYTES + 1)}), "/code"),
            (json!({"code": "x", "extra": 1}), "/extra"),
            (
                json!({"code": "x", "requestedCapabilities": ["ordin_tools"]}),
                "/requestedCapabilities/0",
            ),
            (
                json!({"code": "x", "requestedCapabilities": [1]}),
                "/requestedCapabilities/0",
            ),
            (
                json!({"code": "x", "requestedCapabilities": vec!["a"; 51]}),
                "/requestedCapabilities",
            ),
            (
                json!({"code": "x", "requestedCapabilities": "a"}),
                "/requestedCapabilities",
            ),
            (json!({"code": "x", "dryRun": "yes"}), "/dryRun"),
            (json!({"code": "x", "workSession": 1}), "/workSession"),
            (json!({"code": "x", "files": [{"path": "a"}]}), "/files/0"),
            (
                json!({"code": "x", "files": [{"path": "a", "contentBase64": "", "mode": 1}]}),
                "/files/0/mode",
            ),
            (json!({"code": "x", "files": [7]}), "/files/0"),
            (
                json!({"code": "x", "files": [{"path": 1, "contentBase64": ""}]}),
                "/files/0/path",
            ),
            (
                json!({"code": "x", "inputs": vec![json!({"placeId": place, "path": "a"}); 101]}),
                "/inputs",
            ),
            (
                json!({"code": "x", "inputs": [{"placeId": "nope", "path": "a"}]}),
                "/inputs/0/placeId",
            ),
            (json!({"code": "x", "environment": []}), "/environment"),
            (
                json!({"code": "x", "environment": {"A": 1}}),
                "/environment/A",
            ),
        ];
        for (arguments, at) in cases {
            let p = check(&arguments).expect_err(&arguments.to_string());
            assert_eq!(p.at, at, "{arguments}");
            assert!(p.reason.len() <= 300 && p.reason.contains("is not valid"));
        }
    }

    #[test]
    fn the_schema_is_an_object_schema() {
        let schema = input_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn a_long_reason_is_cut_to_300_characters() {
        let p = problem("", &"f".repeat(400), "r");
        assert_eq!(p.reason.len(), 300);
    }
}
