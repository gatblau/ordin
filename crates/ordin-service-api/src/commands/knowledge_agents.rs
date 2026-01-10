//! Knowledge agents — restated from `KnowledgeQueryClient.md` (SA-RECON-02), in the shapes of
//! ServiceApi.md's amendment January 2026 (ADR-112, ADR-113). `KnowledgeConnectionId`, `KnowledgeAbout`
//! and `SourcesRecordPolicy` are 2C types from `ordin-types`.
//!
//! `knowledge_agents.add` and `knowledge_agents.remove` are person-only (SA-SVC-KA-01).

use ordin_types::common::SetBy;
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::SourcesRecordPolicy;
use ordin_types::WorkspaceId;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::wire::knowledge_query::KnowledgeAbout;
pub use ordin_types::KnowledgeConnectionId;

// The catalogue's shapes (ServiceApi.md, ADR-112 and ADR-113), which the owner's *Service API
// commands* rows restate (KnowledgeQueryClient.md, amended January 2026).

/// The Service API view of one front door (the person's or the company's).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeConnectionView {
    pub connection_id: KnowledgeConnectionId,
    pub name: String,
    pub url: String,
    pub added_by: SetBy,
    /// `null` until the front door's `about` has been read once, or when it keeps no record.
    pub sources_record: Option<SourcesRecordPolicy>,
    pub last_answered_at: Option<Timestamp>,
}

/// `knowledge_agents.list`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeAgentsListInput {
    pub workspace_id: WorkspaceId,
}

/// The workspace's front doors, the person's and the company's, in the order added.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeAgentsListResult {
    pub connections: Vec<KnowledgeConnectionView>,
}

/// `knowledge_agents.add`: `url` an `https` address of at most 2,048 bytes.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeAgentsAddInput {
    pub workspace_id: WorkspaceId,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeAgentsAddResult {
    pub connection_id: KnowledgeConnectionId,
    pub about: KnowledgeAbout,
}

/// `knowledge_agents.remove`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeAgentsRemoveInput {
    pub connection_id: KnowledgeConnectionId,
}

/// `knowledge_agents.details`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeAgentsDetailsInput {
    pub connection_id: KnowledgeConnectionId,
}

/// `about` read fresh from the front door.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeAgentsDetailsResult {
    pub connection: KnowledgeConnectionView,
    pub about: KnowledgeAbout,
}

impl CommandInput for KnowledgeAgentsListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeAgentsAddInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::https_url(&self.url, "/url")
    }
}

impl CommandInput for KnowledgeAgentsRemoveInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeAgentsDetailsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(url: &str) -> KnowledgeAgentsAddInput {
        KnowledgeAgentsAddInput {
            workspace_id: WorkspaceId::new(),
            url: url.to_owned(),
        }
    }

    #[test]
    fn add_url_rules() {
        assert!(add("https://knowledge.procurement.example.org")
            .check()
            .is_ok());
        let long = format!("https://k.example.org/{}", "a".repeat(2048));
        for bad in ["http://knowledge.example.org", "not a url", long.as_str()] {
            let err = add(bad).check().unwrap_err();
            assert_eq!(
                (err.code, err.at.as_str()),
                ("SVC_INPUT_INVALID", "/input/url")
            );
        }
    }

    #[test]
    fn view_shape_and_id_inputs() {
        let view = KnowledgeConnectionView {
            connection_id: KnowledgeConnectionId::new(),
            name: "procurement-knowledge".into(),
            url: "https://knowledge.procurement.example.org".into(),
            added_by: SetBy::Person,
            sources_record: None,
            last_answered_at: None,
        };
        let v = serde_json::to_value(view).unwrap();
        assert!(v["sources_record"].is_null() && v["last_answered_at"].is_null());
        let id = KnowledgeConnectionId::new();
        assert!(KnowledgeAgentsListInput {
            workspace_id: WorkspaceId::new()
        }
        .check()
        .is_ok());
        assert!(KnowledgeAgentsRemoveInput { connection_id: id }
            .check()
            .is_ok());
        assert!(KnowledgeAgentsDetailsInput { connection_id: id }
            .check()
            .is_ok());
    }
}
