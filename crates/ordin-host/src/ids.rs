//! `OrdinConversationId` (chosen here; UUIDv7), the identifier of one Ordin conversation.

use serde::{Deserialize, Serialize};

/// One conversation of Ordin's in a workspace; 36-character lowercase hyphenated UUIDv7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrdinConversationId(pub uuid::Uuid);

impl OrdinConversationId {
    pub fn new() -> Self {
        OrdinConversationId(uuid::Uuid::now_v7())
    }

    /// The Service API's restated form of the same identifier. A UUIDv7 made here always parses
    /// as one; a row whose identifier does not is reported as an unreadable store.
    pub fn wire(
        &self,
    ) -> Result<ordin_service_api::commands::ordin::OrdinConversationId, crate::OhError> {
        ordin_service_api::commands::ordin::OrdinConversationId::parse(&self.to_string())
            .map_err(|_| crate::OhError::DatastoreUnavailable)
    }

    /// From the Service API's form; the text was checked there as a v7 UUID.
    pub fn from_wire(id: &ordin_service_api::commands::ordin::OrdinConversationId) -> Option<Self> {
        id.as_str().parse().ok()
    }
}

impl Default for OrdinConversationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for OrdinConversationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl std::str::FromStr for OrdinConversationId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s).map(OrdinConversationId)
    }
}

pub(crate) type ConversationId = OrdinConversationId;
