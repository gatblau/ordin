//! Approvals — restated from `ApprovalService.md` (SA-RECON-02).
//!
//! `approval.decide` takes the 2C `ApprovalPress` unchanged; its check is written here.

use ordin_types::domain::ApprovalState;
use ordin_types::prelude::*;
use ordin_types::wire::{ActionProposal, ApprovalCard};
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::wire::ApprovalPress;

/// The length of a `CardRef`: 16 random bytes in base64url without padding (2C wire §5).
const CARD_REF_CHARS: usize = 22;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalOutcome {
    pub approval_id: ApprovalId,
    pub state: ApprovalState,
    pub decided_on: String,
}

/// The stored record with its card.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalRecord {
    pub card: ApprovalCard,
    pub proposal: ActionProposal,
    pub state: ApprovalState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<Timestamp>,
}

/// `approval.list_pending`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ApprovalListPendingInput {
    pub workspace_id: Option<WorkspaceId>,
    pub cursor: Option<Cursor>,
}

/// `approval.get`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ApprovalGetInput {
    pub approval_id: ApprovalId,
}

impl CommandInput for ApprovalPress {
    fn check(&mut self) -> Result<(), InputRejection> {
        // `CardRef` keeps its text private; its serde form is the text itself.
        let ok = match serde_json::to_value(&self.card_ref) {
            Ok(serde_json::Value::String(s)) => {
                s.len() == CARD_REF_CHARS
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            }
            _ => false,
        };
        if !ok {
            return Err(rules::reject("/card_ref"));
        }
        Ok(())
    }
}

impl CommandInput for ApprovalListPendingInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for ApprovalGetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(card_ref: &str) -> ApprovalPress {
        serde_json::from_value(serde_json::json!({"card_ref": card_ref, "decision": "approve"}))
            .unwrap()
    }

    #[test]
    fn card_ref_form() {
        assert!(press("q8Zt3VxN0pLk2mRb7sYw1A").check().is_ok());
        for bad in ["short", "q8Zt3VxN0pLk2mRb7sYw1A=", "q8Zt3VxN0pLk2mRb7sYw1+"] {
            assert_eq!(
                press(bad).check().unwrap_err().at,
                "/input/card_ref",
                "{bad}"
            );
        }
    }

    #[test]
    fn list_and_get() {
        assert!(ApprovalListPendingInput {
            workspace_id: None,
            cursor: None
        }
        .check()
        .is_ok());
        let mut long = ApprovalListPendingInput {
            workspace_id: None,
            cursor: Some(Cursor::encode(&"x".repeat(900))),
        };
        assert_eq!(long.check().unwrap_err().code, "SRV_BAD_CURSOR");
        assert!(ApprovalGetInput {
            approval_id: ApprovalId::new()
        }
        .check()
        .is_ok());
        let input: ApprovalListPendingInput =
            serde_json::from_value(serde_json::json!({"workspace_id": null, "cursor": null}))
                .unwrap();
        assert!(input.workspace_id.is_none());
    }

    #[test]
    fn outcome_shape() {
        let json = serde_json::json!({"approval_id": "0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f", "state": "approved", "decided_on": "Asha's laptop"});
        let outcome: ApprovalOutcome = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(outcome).unwrap(), json);
    }
}
