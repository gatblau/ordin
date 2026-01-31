//! SharePoint — restated from `SharePointAdapter.md` (SA-RECON-02), used by **Add to this workspace**.
//!
//! The adapter's item types are `SharePointItem`, `SharePointItemRef` and `SharePointVersion`
//! (SharePointAdapter.md, amended January 2026), distinct from the Confluence Adapter's `RemoteItem`,
//! `RemoteItemRef` and `RemoteVersion`, so each has one name in the generated screen types.

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SharePointLocation {
    pub site_id: String,
    pub site_name: String,
    pub drive_id: String,
    pub drive_name: String,
    /// The folder or file; `None` = the library's root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    /// Path from the library root, `/`-separated, empty for the root.
    pub path: String,
    pub is_folder: bool,
    pub web_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SharePointItemRef {
    pub drive_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SharePointVersion {
    /// Graph `cTag`: changes when the content changes; the "version read" of A-25.
    pub tag: String,
    /// Graph `eTag`: changes on any change; used for `If-Match`.
    pub etag: String,
    /// SharePoint's version label ("1.0", "4.0") when Graph returns one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub changed_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SharePointItem {
    pub item: SharePointItemRef,
    pub name: String,
    /// Relative to the listed location, `/`-separated; folders end with `/`.
    pub path: String,
    pub is_folder: bool,
    pub size_bytes: u64,
    /// None for folders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<SharePointVersion>,
}

/// `sharepoint.find`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SharepointFindInput {
    pub query: String,
    pub limit: u32,
}

/// `sharepoint.list`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SharepointListInput {
    pub location: SharePointLocation,
    pub cursor: Option<Cursor>,
}

impl CommandInput for SharepointFindInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.query, 1, 200, "/query")?;
        rules::range(self.limit, 1, 50, "/limit")
    }
}

impl CommandInput for SharepointListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_limits() {
        assert!(SharepointFindInput {
            query: "q".into(),
            limit: 50
        }
        .check()
        .is_ok());
        assert_eq!(
            SharepointFindInput {
                query: String::new(),
                limit: 1
            }
            .check()
            .unwrap_err()
            .at,
            "/input/query"
        );
        assert_eq!(
            SharepointFindInput {
                query: "q".into(),
                limit: 51
            }
            .check()
            .unwrap_err()
            .at,
            "/input/limit"
        );
    }

    #[test]
    fn list_cursor() {
        let location = SharePointLocation {
            site_id: "s".into(),
            site_name: "Finance".into(),
            drive_id: "d".into(),
            drive_name: "Documents".into(),
            item_id: None,
            path: String::new(),
            is_folder: true,
            web_url: "https://acme.sharepoint.com/sites/finance".into(),
        };
        let mut input = SharepointListInput {
            location,
            cursor: Some(Cursor::encode(&"x".repeat(900))),
        };
        assert_eq!(input.check().unwrap_err().code, "SRV_BAD_CURSOR");
        input.cursor = None;
        assert!(input.check().is_ok());
    }
}
