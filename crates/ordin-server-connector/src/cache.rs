//! The read cache of one server (ServerConnector.md Internal Logic 5): the last reply of each read
//! command, kept in memory only, least recently used out first once the byte bound is reached.

use std::collections::HashMap;

use bytes::Bytes;
use sha2::{Digest, Sha256};

/// `(command name, sha256 of the normalised input JSON)`.
type Key = (String, [u8; 32]);

struct Entry {
    reply: Bytes,
    used: u64,
}

pub(crate) struct ReadCache {
    max_bytes: usize,
    bytes: usize,
    tick: u64,
    entries: HashMap<Key, Entry>,
}

impl ReadCache {
    pub(crate) fn new(max_bytes: usize) -> Self {
        ReadCache {
            max_bytes,
            bytes: 0,
            tick: 0,
            entries: HashMap::new(),
        }
    }

    /// The cache key of a command envelope: its name and the hash of its `input` in canonical form. An
    /// envelope that is not JSON hashes as no input.
    pub(crate) fn key(name: &str, envelope_json: &[u8]) -> Key {
        let input = serde_json::from_slice::<serde_json::Value>(envelope_json)
            .ok()
            .and_then(|mut v| v.get_mut("input").map(serde_json::Value::take))
            .unwrap_or(serde_json::Value::Null);
        let canonical = ordin_types::encoding::canonical_json(&input);
        (name.to_owned(), Sha256::digest(canonical).into())
    }

    pub(crate) fn get(&mut self, key: &Key) -> Option<Bytes> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|e| {
            e.used = tick;
            e.reply.clone()
        })
    }

    /// Stores a reply; a reply larger than the whole bound is not kept.
    pub(crate) fn put(&mut self, key: Key, reply: Bytes) {
        if reply.len() > self.max_bytes {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.reply.len();
        }
        while self.bytes + reply.len() > self.max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(gone) = self.entries.remove(&oldest) {
                self.bytes -= gone.reply.len();
            }
        }
        self.tick += 1;
        self.bytes += reply.len();
        self.entries.insert(
            key,
            Entry {
                reply,
                used: self.tick,
            },
        );
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ignores_object_order_and_request_id() {
        let a = ReadCache::key("w.list", br#"{"request_id":"1","input":{"a":1,"b":2}}"#);
        let b = ReadCache::key("w.list", br#"{"request_id":"2","input":{"b":2,"a":1}}"#);
        let c = ReadCache::key("w.list", br#"{"request_id":"2","input":{"b":3,"a":1}}"#);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(
            ReadCache::key("x", b"not json").1,
            ReadCache::key("x", b"{}").1
        );
    }

    #[test]
    fn evicts_least_recently_used_and_skips_oversized() {
        let mut cache = ReadCache::new(10);
        let k = |n: &str| (n.to_owned(), [0u8; 32]);
        cache.put(k("a"), Bytes::from_static(b"1234"));
        cache.put(k("b"), Bytes::from_static(b"1234"));
        assert!(cache.get(&k("a")).is_some());
        cache.put(k("c"), Bytes::from_static(b"1234"));
        assert!(cache.get(&k("b")).is_none(), "b was least recently used");
        assert!(cache.get(&k("a")).is_some());
        cache.put(k("big"), Bytes::from_static(b"12345678901"));
        assert!(cache.get(&k("big")).is_none());
        cache.put(k("a"), Bytes::from_static(b"12"));
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert_eq!(cache.len(), 0);
    }
}
