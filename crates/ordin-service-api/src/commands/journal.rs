//! Journal entries and external actions — restated from `JournalAndArtefactStore.md` (SA-RECON-02).
//!
//! The Journal's *Service API commands* rows are the definitive inputs and results; the entry types are
//! its *Supporting types*, and `ExternalAction` with its key and state are 2C types from `ordin-types`.

use ordin_types::time::Timestamp;
use ordin_types::{CrewRunId, Cursor, RunId, WorkspaceId};
use serde::{Deserialize, Deserializer, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{ExternalAction, ExternalActionState};
pub use ordin_types::ExternalActionKey;

/// UUIDv7 (SA-ID-01 "journal record"), as the 36-character lowercase hyphenated string.
// SPEC-GAP-10: the Journal types `JournalEntryId(pub uuid::Uuid)`; this crate has no `uuid`
// dependency (its Cargo.toml names none), so the identifier is held as its validated wire string. The
// serialised form is the same.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct JournalEntryId(pub String);

impl<'de> Deserialize<'de> for JournalEntryId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        ordin_types::id::parse_v7(&raw).map_err(serde::de::Error::custom)?;
        Ok(JournalEntryId(raw))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct JournalEntry {
    pub id: JournalEntryId,
    pub kind: String,
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crew_run_id: Option<CrewRunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    pub summary: String,
    pub data: serde_json::Value,
    pub recorded_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EntryFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// `journal.list_entries`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct JournalListEntriesInput {
    pub filter: EntryFilter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

/// `journal.list_external_actions`: `state` is `"unknown_outcome"` or absent.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct JournalListExternalActionsInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ExternalActionState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

/// `journal.mark_arrived`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct JournalMarkArrivedInput {
    pub action_key: ExternalActionKey,
}

/// `journal.run_external_actions`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct JournalRunExternalActionsInput {
    pub run_id: RunId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct JournalRunExternalActionsResult {
    pub items: Vec<ExternalAction>,
}

impl CommandInput for JournalListEntriesInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for JournalListExternalActionsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        // The row's only state value is `unknown_outcome`.
        if matches!(self.state, Some(s) if s != ExternalActionState::UnknownOutcome) {
            return Err(rules::reject("/state"));
        }
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for JournalMarkArrivedInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for JournalRunExternalActionsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_action_state_filter() {
        let mut ok = JournalListExternalActionsInput {
            state: Some(ExternalActionState::UnknownOutcome),
            cursor: None,
        };
        assert!(ok.check().is_ok());
        let mut bad = JournalListExternalActionsInput {
            state: Some(ExternalActionState::Sent),
            cursor: None,
        };
        let err = bad.check().unwrap_err();
        assert_eq!(
            (err.code, err.at.as_str()),
            ("SVC_INPUT_INVALID", "/input/state")
        );
        let mut long = JournalListExternalActionsInput {
            state: None,
            cursor: Some(Cursor::encode(&"x".repeat(900))),
        };
        assert_eq!(long.check().unwrap_err().code, "SRV_BAD_CURSOR");
    }

    #[test]
    fn entries_and_ids() {
        let mut input: JournalListEntriesInput =
            serde_json::from_value(serde_json::json!({"filter": {"kind": "repair_done"}})).unwrap();
        assert!(input.check().is_ok());
        assert!(serde_json::from_value::<JournalListEntriesInput>(
            serde_json::json!({"filter": {}, "extra": 1})
        )
        .is_err());
        let id: JournalEntryId =
            serde_json::from_value(serde_json::json!("0192a1c2-3344-7556-8677-8899aabbccdd"))
                .unwrap();
        assert_eq!(id.0, "0192a1c2-3344-7556-8677-8899aabbccdd");
        assert!(serde_json::from_value::<JournalEntryId>(serde_json::json!("abc")).is_err());
        assert!(JournalMarkArrivedInput {
            action_key: ExternalActionKey::new()
        }
        .check()
        .is_ok());
        assert!(JournalRunExternalActionsInput {
            run_id: RunId::new()
        }
        .check()
        .is_ok());
    }
}
