//! One open harness session: its secret, replay set, tool-use map and event sender (B, C.6, C.8,
//! E.1, F.1). All of it is memory only and gone when the session closes or the process ends.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::Mutex;

use hmac::{Hmac, Mac};
use ordin_types::encoding::{b64url_decode_exact, canonical_json};
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::{HookCallId, ToolRequestId};
use sha2::Sha256;
use tokio::sync::broadcast;
use zeroize::Zeroizing;

use crate::consts::{EVENT_CAPACITY, REPLAY_CAPACITY, TOOL_USE_CAPACITY};
use crate::types::{HookSessionEvent, HookSessionSpec};

/// A set that remembers the most recent `capacity` keys, oldest dropped first.
struct Recent<K: Hash + Eq + Clone> {
    capacity: usize,
    order: VecDeque<K>,
    keys: HashSet<K>,
}

impl<K: Hash + Eq + Clone> Recent<K> {
    fn new(capacity: usize) -> Self {
        Recent {
            capacity,
            order: VecDeque::new(),
            keys: HashSet::new(),
        }
    }

    /// True when the key was not there; it is then remembered.
    fn insert(&mut self, key: K) -> bool {
        if !self.keys.insert(key.clone()) {
            return false;
        }
        self.order.push_back(key);
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.keys.remove(&oldest);
            }
        }
        true
    }
}

struct State {
    seen: Recent<HookCallId>,
    tool_uses: HashMap<String, (ToolRequestId, ToolOrigin)>,
    tool_use_order: VecDeque<String>,
    /// The write claim of each call that has one (FileService.md Internal Logic 6a). One left here
    /// for want of an after-tool event settles when the run ends.
    write_claims: HashMap<String, String>,
}

pub(crate) struct Session {
    pub spec: HookSessionSpec,
    secret: Zeroizing<[u8; 32]>,
    state: Mutex<State>,
    events: Mutex<Option<broadcast::Sender<HookSessionEvent>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Session {
    pub(crate) fn new(spec: HookSessionSpec, secret: [u8; 32]) -> Session {
        let (sender, _) = broadcast::channel(EVENT_CAPACITY);
        Session {
            spec,
            secret: Zeroizing::new(secret),
            state: Mutex::new(State {
                seen: Recent::new(REPLAY_CAPACITY),
                tool_uses: HashMap::new(),
                tool_use_order: VecDeque::new(),
                write_claims: HashMap::new(),
            }),
            events: Mutex::new(Some(sender)),
        }
    }

    /// C.6: HMAC-SHA256 over the canonical JSON of `message` under the session's secret; `mac` is
    /// base64url without padding. A text that does not decode to 32 bytes counts as a mismatch. The
    /// comparison is constant time (`Mac::verify_slice`).
    // spec: ordin-desktop/components/HookRelay#SA-HOOK-02
    pub(crate) fn mac_matches(&self, message: &serde_json::Value, mac: &str) -> bool {
        let Ok(given) = b64url_decode_exact(mac, 32, "HOOK_UNAUTHENTICATED") else {
            return false;
        };
        let Ok(mut expected) = <Hmac<Sha256> as Mac>::new_from_slice(&self.secret[..]) else {
            return false;
        };
        expected.update(&canonical_json(message));
        expected.verify_slice(&given).is_ok()
    }

    /// C.8: true when `call_id` has not been seen in this session; it is then remembered.
    pub(crate) fn first_sight(&self, call_id: HookCallId) -> bool {
        lock(&self.state).seen.insert(call_id)
    }

    /// D.3: remembers which tool request an allowed call became, for the after-tool event.
    pub(crate) fn remember_tool_use(
        &self,
        harness_tool_use_id: &str,
        request: ToolRequestId,
        origin: ToolOrigin,
    ) {
        let mut state = lock(&self.state);
        if state
            .tool_uses
            .insert(harness_tool_use_id.to_owned(), (request, origin))
            .is_none()
        {
            state
                .tool_use_order
                .push_back(harness_tool_use_id.to_owned());
        }
        while state.tool_use_order.len() > TOOL_USE_CAPACITY {
            if let Some(oldest) = state.tool_use_order.pop_front() {
                state.tool_uses.remove(&oldest);
            }
        }
    }

    /// E.1: the entry is dropped when found.
    pub(crate) fn take_tool_use(
        &self,
        harness_tool_use_id: &str,
    ) -> Option<(ToolRequestId, ToolOrigin)> {
        let mut state = lock(&self.state);
        let found = state.tool_uses.remove(harness_tool_use_id);
        if found.is_some() {
            state.tool_use_order.retain(|id| id != harness_tool_use_id);
        }
        found
    }

    pub(crate) fn remember_write_claim(&self, harness_tool_use_id: &str, claim: String) {
        lock(&self.state)
            .write_claims
            .insert(harness_tool_use_id.to_owned(), claim);
    }

    pub(crate) fn take_write_claim(&self, harness_tool_use_id: &str) -> Option<String> {
        lock(&self.state).write_claims.remove(harness_tool_use_id)
    }

    /// F.1: a new receiver; `None` once the session is closed.
    pub(crate) fn subscribe(&self) -> Option<broadcast::Receiver<HookSessionEvent>> {
        lock(&self.events).as_ref().map(|sender| sender.subscribe())
    }

    /// F.1: sends one event; nobody listening is not an error.
    pub(crate) fn emit(&self, event: HookSessionEvent) {
        if let Some(sender) = lock(&self.events).as_ref() {
            let _ = sender.send(event);
        }
    }

    /// I: drops the sender, which ends every stream. The secret is wiped when the last holder of the
    /// session lets go.
    pub(crate) fn close(&self) {
        lock(&self.events).take();
        let mut state = lock(&self.state);
        state.seen = Recent::new(REPLAY_CAPACITY);
        state.tool_uses.clear();
        state.tool_use_order.clear();
        state.write_claims.clear();
    }
}
