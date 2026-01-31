//! base64url without padding, and the one canonical JSON of the workspace.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;

use crate::TypesError;

/// base64url without padding (RFC 4648 §5).
pub fn b64url_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Refuses padding, characters outside the URL-safe alphabet, and a decoded length != `expected_len`.
pub fn b64url_decode_exact(
    raw: &str,
    expected_len: usize,
    code: &'static str,
) -> Result<Vec<u8>, TypesError> {
    // The no-padding engine refuses `=` and every character outside `A-Z a-z 0-9 - _`.
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| TypesError::Rule { code })?;
    if bytes.len() != expected_len {
        return Err(TypesError::Rule { code });
    }
    Ok(bytes)
}

/// The one canonical JSON of the workspace (Phase 4 Logging defect, resolved): object keys sorted
/// by their UTF-8 bytes at every level, no insignificant white space, strings escaped and numbers
/// written as `serde_json` writes them. Used for the argument digest (Logging R-3), the hook MAC
/// (Hook Relay H, Ordin Hook E) and every hash the Agent Model takes (`ordin_agent_model::canonical_json`
/// re-exports this function).
pub fn canonical_json(value: &serde_json::Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            // `str` orders by its UTF-8 bytes, which is the order the canonical form needs.
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push(b'{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_scalar(&Value::String(key.clone()), out);
                out.push(b':');
                write_canonical(item, out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        scalar => write_scalar(scalar, out),
    }
}

/// A string, number, boolean or null exactly as `serde_json` writes it (compact form).
fn write_scalar(value: &serde_json::Value, out: &mut Vec<u8>) {
    // Writing a scalar `Value` into a `Vec` cannot fail: it has no map with non-string keys and the
    // writer never errors. An error would leave the scalar out rather than panic.
    let _ = serde_json::to_writer(&mut *out, value);
}
