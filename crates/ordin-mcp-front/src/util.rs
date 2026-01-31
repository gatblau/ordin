//! Small helpers shared by the modules of this crate.

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Locks a mutex; a poisoned lock still yields its data, because every guarded value here is
/// plain state that stays consistent between statements.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lower-case hexadecimal of a byte slice.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// SHA-256 of the canonical JSON of `value` (object keys sorted), as lower-case hexadecimal. The
/// log line of a call carries this digest and never the arguments (Internal Logic 21).
pub(crate) fn arguments_digest(value: &Value) -> String {
    let mut text = String::new();
    canonical(value, &mut text);
    hex(&Sha256::digest(text.as_bytes()))
}

fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                if let Some(inner) = map.get(key) {
                    canonical(inner, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Escapes one JSON Pointer reference token (RFC 6901).
pub(crate) fn pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// The first `max` characters of `text`.
pub(crate) fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn digest_ignores_key_order() {
        let a = json!({"b": 1, "a": [1, 2, {"y": 1, "x": 2}]});
        let b = json!({"a": [1, 2, {"x": 2, "y": 1}], "b": 1});
        assert_eq!(arguments_digest(&a), arguments_digest(&b));
        assert_eq!(arguments_digest(&a).len(), 64);
        assert_ne!(arguments_digest(&a), arguments_digest(&json!({"a": 1})));
    }

    #[test]
    fn pointer_tokens_are_escaped() {
        assert_eq!(pointer_token("a/b~c"), "a~1b~0c");
        assert_eq!(clip("héllo", 2), "hé");
    }
}
