//! E. The request's MAC: HMAC-SHA256 over the canonical JSON of `message`, base64url without padding.

use hmac::{Hmac, Mac};
use ordin_types::encoding::{b64url_encode, canonical_json};
use ordin_types::wire::hook::HookMessage;
use sha2::Sha256;

pub fn sign(secret: &[u8; 32], message: &HookMessage) -> String {
    // A `HookMessage` is made of strings, booleans and JSON values, so conversion cannot fail; the
    // fallback keeps the function total and yields a MAC the relay rejects.
    let value = serde_json::to_value(message).unwrap_or(serde_json::Value::Null);
    // HMAC accepts a key of any length, so the `else` arm cannot run.
    let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(secret) else {
        return String::new();
    };
    mac.update(&canonical_json(&value));
    b64url_encode(&mac.finalize().into_bytes())
}
