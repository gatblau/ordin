//! Confluence — restated from `ConfluenceAdapter.md` (SA-RECON-02).
//!
//! The adapter's reading and publishing types, as its *Service API commands* table names them. Every
//! command of this group is `Local`: the ordin server has no Confluence Adapter (SA-RECON-03).

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

/// Most items one **Publish checked items…** may carry (Error Table: "more than 50 checked items").
const MAX_CHECKED_ITEMS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ConfluenceLocation {
    pub space_key: String,
    pub space_name: String,
    /// None = the whole space.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
    pub title: String,
    pub web_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteItemRef {
    Page {
        page_id: String,
    },
    Attachment {
        page_id: String,
        attachment_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RemoteVersion {
    /// The Confluence version number as text ("7"); the "version read" of A-25.
    pub tag: String,
    pub changed_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RemoteItem {
    pub item: RemoteItemRef,
    pub title: String,
    /// Path in the working copy relative to the link root.
    pub path: String,
    pub has_children: bool,
    pub size_bytes: Option<u64>,
    pub version: RemoteVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ReadyItem {
    pub confluence_link_id: ConfluenceLinkId,
    pub kind: ReadyKind,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_copy_id: Option<PublishedCopyId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadyKind {
    New,
    Renamed,
    Removed,
    // A published file whose change waits for the person's card (ConfluenceAdapter.md amendment
    // 2026-01-24). A plain comment, so the generated schema stays one flat enum.
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-03
    Changed,
}

/// `confluence.find`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ConfluenceFindInput {
    pub query: String,
    pub limit: u32,
}

/// `confluence.list`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ConfluenceListInput {
    pub location: ConfluenceLocation,
    pub cursor: Option<Cursor>,
}

/// `confluence.ready_to_publish`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ConfluenceReadyToPublishInput {
    pub confluence_link_id: ConfluenceLinkId,
}

/// `confluence.publish_checked`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ConfluencePublishCheckedInput {
    pub confluence_link_id: ConfluenceLinkId,
    pub items: Vec<ReadyItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ConfluencePublishCheckedResult {
    pub approval_id: ApprovalId,
}

/// `confluence.keep_mine`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ConfluenceKeepMineInput {
    pub published_copy_id: PublishedCopyId,
}

impl CommandInput for ConfluenceFindInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.query, 1, 200, "/query")?;
        rules::range(self.limit, 1, 25, "/limit")
    }
}

impl CommandInput for ConfluenceListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for ConfluenceReadyToPublishInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for ConfluencePublishCheckedInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::count(&self.items, 0, MAX_CHECKED_ITEMS, "/items")
    }
}

impl CommandInput for ConfluenceKeepMineInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

/// The person a pasted API token belongs to (`confluence.connect_token`, `confluence.connect_with_jira`;
/// SVC-CONF-TOKEN-1). Never the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ConfluenceUser {
    /// 1–128 characters.
    pub account_id: String,
    /// 1–255 characters.
    pub display_name: String,
}

/// `confluence.connect_token`: a pasted Atlassian API token, checked with
/// `GET /wiki/rest/api/user/current`, then kept through Identity (SVC-CONF-TOKEN-1; amended January 2026;
/// user decision; Approved by the user January 2026). The input carries a secret: it is never logged,
/// journalled or echoed, so its `Debug` hides the token.
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ConfluenceConnectTokenInput {
    /// The site's host, `{name}.atlassian.net`.
    pub site: String,
    /// 3–254 characters.
    pub email: String,
    /// 1–1,024 characters; never logged.
    pub token: String,
}

impl std::fmt::Debug for ConfluenceConnectTokenInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfluenceConnectTokenInput")
            .field("site", &self.site)
            .field("email", &self.email)
            .field("token", &"***")
            .finish()
    }
}

impl CommandInput for ConfluenceConnectTokenInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::host(&self.site, "/site")?;
        rules::email(&self.email, 3, 254, "/email")?;
        rules::text(&self.token, 1, 1024, "/token")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connect(site: &str, email: &str, token: &str) -> Option<String> {
        ConfluenceConnectTokenInput {
            site: site.into(),
            email: email.into(),
            token: token.into(),
        }
        .check()
        .err()
        .map(|r| r.at)
    }

    // spec: ordin-desktop/components/ServiceApi#SVC-CONF-TOKEN-1
    #[test]
    fn connect_token_rules() {
        assert_eq!(connect("acme.atlassian.net", "sam@acme.com", "t"), None);
        assert_eq!(
            connect("bad host", "sam@acme.com", "t").as_deref(),
            Some("/input/site")
        );
        assert_eq!(
            connect("acme.atlassian.net", "sam", "t").as_deref(),
            Some("/input/email")
        );
        assert_eq!(
            connect("acme.atlassian.net", "sam@acme.com", "").as_deref(),
            Some("/input/token")
        );
        assert_eq!(
            connect("acme.atlassian.net", "sam@acme.com", &"t".repeat(1025)).as_deref(),
            Some("/input/token")
        );
    }

    #[test]
    fn the_token_is_never_in_debug() {
        let input = ConfluenceConnectTokenInput {
            site: "a.atlassian.net".into(),
            email: "a@b.c".into(),
            token: "s3cret-token".into(),
        };
        assert!(!format!("{input:?}").contains("s3cret"));
    }

    fn location() -> ConfluenceLocation {
        ConfluenceLocation {
            space_key: "OPS".into(),
            space_name: "Operations".into(),
            page_id: None,
            title: "Operations".into(),
            web_url: "https://acme.atlassian.net/wiki/spaces/OPS".into(),
        }
    }

    #[test]
    fn find_limits() {
        let cases = [
            ("q", 1, None),
            ("", 1, Some("/input/query")),
            (&*"x".repeat(201), 1, Some("/input/query")),
            ("q", 0, Some("/input/limit")),
            ("q", 26, Some("/input/limit")),
        ];
        for (query, limit, want) in cases {
            let got = ConfluenceFindInput {
                query: query.into(),
                limit,
            }
            .check();
            assert_eq!(got.err().map(|r| r.at), want.map(String::from));
        }
    }

    #[test]
    fn list_cursor_and_items() {
        let mut ok = ConfluenceListInput {
            location: location(),
            cursor: None,
        };
        assert!(ok.check().is_ok());
        let mut long = ConfluenceListInput {
            location: location(),
            cursor: Some(Cursor::encode(&"x".repeat(900))),
        };
        assert_eq!(long.check().unwrap_err().code, "SRV_BAD_CURSOR");
        let item = ReadyItem {
            confluence_link_id: ConfluenceLinkId::new(),
            kind: ReadyKind::New,
            path: "a.md".into(),
            from_path: None,
            published_copy_id: None,
        };
        let mut many = ConfluencePublishCheckedInput {
            confluence_link_id: ConfluenceLinkId::new(),
            items: vec![item; 51],
        };
        assert_eq!(many.check().unwrap_err().at, "/input/items");
        assert!(ConfluenceReadyToPublishInput {
            confluence_link_id: ConfluenceLinkId::new()
        }
        .check()
        .is_ok());
        assert!(ConfluenceKeepMineInput {
            published_copy_id: PublishedCopyId::new()
        }
        .check()
        .is_ok());
    }

    #[test]
    fn remote_item_shape() {
        let json = serde_json::json!({"kind": "attachment", "page_id": "1", "attachment_id": "2"});
        let item: RemoteItemRef = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&item).unwrap(), json);
        let loc = serde_json::to_value(location()).unwrap();
        assert!(loc.get("page_id").is_none());
    }
}
