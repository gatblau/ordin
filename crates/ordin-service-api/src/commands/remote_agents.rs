//! Remote agents — restated from `OrganisationProfile.md` (SA-RECON-02). `RemoteAgent` is the 2C
//! entity from `ordin-types`.

use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::RemoteAgent;

/// `remote_agents.list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteAgentsListInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RemoteAgentsListResult {
    pub items: Vec<RemoteAgent>,
}

impl CommandInput for RemoteAgentsListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        assert!(RemoteAgentsListInput {}.check().is_ok());
        assert!(
            serde_json::from_value::<RemoteAgentsListInput>(serde_json::json!({"a": 1})).is_err()
        );
        let empty = RemoteAgentsListResult { items: vec![] };
        assert_eq!(
            serde_json::to_value(empty).unwrap(),
            serde_json::json!({"items": []})
        );
    }
}
