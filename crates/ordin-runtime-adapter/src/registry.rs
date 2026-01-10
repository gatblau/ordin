//! The adapters a program constructed, keyed by kind (Internal Logic 9).

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter::RuntimeAdapter;
use crate::error::RegistryError;
use crate::{AdapterKind, AdapterVerb};

/// The order `kinds` and `verbs_not_offered` report in, so the start-up report is stable.
const KIND_ORDER: [AdapterKind; 4] = [
    AdapterKind::ClaudeLocal,
    AdapterKind::CopilotLocal,
    AdapterKind::HostedAgent,
    AdapterKind::SlackThread,
];

/// The adapters a program constructed, kept by the Session Supervisor (filled through
/// `SessionSupervisor::register_adapter`).
#[derive(Default, Clone)]
pub struct AdapterRegistry {
    adapters: HashMap<AdapterKind, Arc<dyn RuntimeAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Refuses a second adapter of the same kind (`RA_ADAPTER_DUPLICATE`).
    pub fn register(&mut self, adapter: Arc<dyn RuntimeAdapter>) -> Result<(), RegistryError> {
        let kind = adapter.kind();
        if self.adapters.contains_key(&kind) {
            return Err(RegistryError::Duplicate { kind });
        }
        self.adapters.insert(kind, adapter);
        Ok(())
    }

    /// `RA_ADAPTER_UNKNOWN` when no adapter of `kind` was registered in this program.
    pub fn get(&self, kind: AdapterKind) -> Result<Arc<dyn RuntimeAdapter>, RegistryError> {
        self.adapters
            .get(&kind)
            .cloned()
            .ok_or(RegistryError::Unknown { kind })
    }

    /// The registered kinds, in the order `ClaudeLocal`, `CopilotLocal`, `HostedAgent`,
    /// `SlackThread`.
    pub fn kinds(&self) -> Vec<AdapterKind> {
        KIND_ORDER
            .into_iter()
            .filter(|kind| self.adapters.contains_key(kind))
            .collect()
    }

    /// Every (kind, verb) pair not offered, for the start-up report's `verb_not_offered` items.
    /// Kinds in the order of [`AdapterRegistry::kinds`]; each adapter's verbs in the order it
    /// declares them.
    pub fn verbs_not_offered(&self) -> Vec<(AdapterKind, AdapterVerb)> {
        KIND_ORDER
            .into_iter()
            .filter_map(|kind| self.adapters.get(&kind).map(|adapter| (kind, adapter)))
            .flat_map(|(kind, adapter)| {
                adapter
                    .unsupported_verbs()
                    .iter()
                    .map(move |verb| (kind, *verb))
            })
            .collect()
    }
}
