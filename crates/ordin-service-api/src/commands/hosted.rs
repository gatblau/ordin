//! Hosted agents' approver cards — restated from `HostedAgentAdapter.md`, through the desktop program
//! (SA-CLOSE-05).
//!
//! The program calls `HostedAgentAdapter::decide_approver_card(server_or_agent, approval_id, decision,
//! note)`; the adapter's refusals (`HAA_CARD_DECIDED`, `HAA_NO_ACCESS`, `HAA_CARD_UNKNOWN`) pass through.

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

/// The longest note a person may add to an approver decision (`note` 0–2,000 characters).
const NOTE_MAX_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApproverDecision {
    Approve,
    Reject,
}

/// `hosted.decide_card`. `note` is never logged.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HostedDecideCardInput {
    pub server_or_agent: RemoteAgentId,
    pub approval_id: ApprovalId,
    pub decision: ApproverDecision,
    pub note: Option<String>,
}

/// The decision the hosted agent accepted.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HostedDecideCardResult {
    pub state: HostedDecideCardState,
}

// SPEC-GAP-6: ServiceApi.md writes the result as `{ state: "approved" | "rejected" }` without naming
// the enum; it is named here after the mechanical result name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostedDecideCardState {
    Approved,
    Rejected,
}

impl CommandInput for HostedDecideCardInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::opt_text(&self.note, 0, NOTE_MAX_CHARS, "/note")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(note: Option<String>) -> HostedDecideCardInput {
        HostedDecideCardInput {
            server_or_agent: RemoteAgentId::new(),
            approval_id: ApprovalId::new(),
            decision: ApproverDecision::Approve,
            note,
        }
    }

    #[test]
    fn note_limit() {
        assert!(input(None).check().is_ok());
        assert!(input(Some(String::new())).check().is_ok());
        assert_eq!(
            input(Some("x".repeat(2_001))).check().unwrap_err().at,
            "/input/note"
        );
    }

    #[test]
    fn result_shape() {
        let r = HostedDecideCardResult {
            state: HostedDecideCardState::Approved,
        };
        assert_eq!(
            serde_json::to_value(r).unwrap(),
            serde_json::json!({"state": "approved"})
        );
    }
}
