//! Search — restated from `HomeAndSearch.md` (SA-RECON-02).
//!
//! The one search command of the Home and Search module, answered from local data within the `Search`
//! budget (A-21).

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::home::OpenTarget;
use super::rules;
use crate::api::{CommandInput, InputRejection};

/// `QUERY_MAX`: the longest search text, in characters (HomeAndSearch.md *Constants*).
const QUERY_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoToKind {
    Workspace,
    Agent,
    Workflow,
    Dashboard,
    KnowledgePage,
    Conversation,
    Document,
}

/// A *Go to* result, or an entry of the actions group.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct GoToResult {
    pub kind: GoToKind,
    pub label: String,
    pub detail: Option<String>,
    pub workspace_id: Option<WorkspaceId>,
    pub opens: OpenTarget,
}

/// A *Found in pages and documents* result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FoundResult {
    pub label: String,
    pub snippet: Option<String>,
    pub workspace_id: WorkspaceId,
    pub opens: OpenTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SearchAnswer {
    pub query: String,
    pub go_to: Vec<GoToResult>,
    pub found: Vec<FoundResult>,
    pub actions: Vec<GoToResult>,
    /// "Searched 41,200 of 58,000 documents" while indexes are still building (11 §5.6).
    pub coverage: Option<(u64, u64)>,
    pub complete: bool,
}

/// `search.query`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SearchQueryInput {
    pub query: String,
    pub current_workspace_id: Option<WorkspaceId>,
}

impl CommandInput for SearchQueryInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        // Owner's row: search text empty or over 200 characters (`HOME_QUERY_INVALID`); a text of
        // spaces only reaches the owner, which trims it.
        rules::text(&self.query, 1, QUERY_MAX_CHARS, "/query")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_length() {
        let input = |q: &str| SearchQueryInput {
            query: q.into(),
            current_workspace_id: None,
        };
        assert!(input("Kestrel").check().is_ok());
        assert!(input(&"é".repeat(200)).check().is_ok());
        for bad in ["", &"x".repeat(201)] {
            let err = input(bad).check().unwrap_err();
            assert_eq!(
                (err.code, err.at.as_str()),
                ("SVC_INPUT_INVALID", "/input/query")
            );
        }
    }

    #[test]
    fn answer_has_the_owner_shape() {
        let answer = SearchAnswer {
            query: "Kestrel".into(),
            go_to: vec![],
            found: vec![],
            actions: vec![],
            coverage: Some((41_200, 58_000)),
            complete: true,
        };
        let json = serde_json::to_value(&answer).unwrap();
        assert_eq!(json["coverage"], serde_json::json!([41200, 58000]));
    }
}
