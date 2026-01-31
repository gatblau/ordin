//! Reports — restated from `JournalAndArtefactStore.md` (SA-RECON-02).
//!
//! `Report` is the 2C entity from `ordin-types`; `ReportSummary` is the Journal's supporting type.

use ordin_types::time::Timestamp;
use ordin_types::{Cursor, ReportId, RunId, WorkspaceId};
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::Report;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ReportSummary {
    pub id: ReportId,
    pub run_id: RunId,
    pub title: String,
    pub as_of: Timestamp,
}

/// `reports.get`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReportsGetInput {
    pub report_id: ReportId,
}

/// `reports.list`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReportsListInput {
    pub workspace_id: WorkspaceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

impl CommandInput for ReportsGetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for ReportsListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs() {
        assert!(ReportsGetInput {
            report_id: ReportId::new()
        }
        .check()
        .is_ok());
        let mut ok = ReportsListInput {
            workspace_id: WorkspaceId::new(),
            cursor: None,
        };
        assert!(ok.check().is_ok());
        let mut long = ReportsListInput {
            workspace_id: WorkspaceId::new(),
            cursor: Some(Cursor::encode(&"x".repeat(900))),
        };
        let err = long.check().unwrap_err();
        assert_eq!(
            (err.code, err.at.as_str()),
            ("SRV_BAD_CURSOR", "/input/cursor")
        );
    }
}
