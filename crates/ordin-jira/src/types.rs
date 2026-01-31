//! The public data types of the Jira Adapter (JiraAdapter.md *Public Interface*).

use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use serde::{Deserialize, Serialize};

use crate::JiraAdapter;

/// The collaborators the program hands the adapter.
pub struct JiraDeps {
    pub journal: std::sync::Arc<ordin_journal::Journal>,
    pub identity: std::sync::Arc<ordin_identity::IdentityService>,
    /// The shared client built by the program (SA-HTTP-02).
    pub http: reqwest::Client,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraProjectRef {
    /// Host, `{name}.atlassian.net`.
    pub site: String,
    /// `ABC`.
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraIssueRef {
    pub site: String,
    /// `ABC-12`.
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraProject {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraUser {
    /// 1-128 characters.
    pub account_id: String,
    /// 1-255 characters.
    pub display_name: String,
    /// Absent under the person's privacy settings (C-88).
    // spec: ordin-desktop/components/JiraAdapter#C-88
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraStatusCategory {
    ToDo,
    InProgress,
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraIssueSummary {
    pub issue: JiraIssueRef,
    pub id: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<JiraUser>,
    pub status_category: JiraStatusCategory,
    pub labels: Vec<String>,
    pub updated: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraCodeBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraComment {
    pub id: String,
    pub author: JiraUser,
    pub created: Timestamp,
    pub updated: Timestamp,
    /// The whole body as Markdown, code blocks included as fenced blocks (Internal Logic 8.2).
    pub body_markdown: String,
    /// Every code block of the body, in document order, as written (no Markdown escaping).
    pub code_blocks: Vec<JiraCodeBlock>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraAttachment {
    pub id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub created: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraLinkedIssue {
    /// Jira's own wording for the link from this issue, e.g. "blocks", "is blocked by", "relates to".
    pub relation: String,
    pub issue: JiraIssueSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraIssueContent {
    pub summary: JiraIssueSummary,
    /// "" when the issue has no description.
    pub description_markdown: String,
    /// All pages, creation order (C-86).
    pub comments: Vec<JiraComment>,
    pub attachments: Vec<JiraAttachment>,
    /// As listed on the issue; those issues are not read.
    pub subtasks: Vec<JiraIssueSummary>,
    /// As listed on the issue; those issues are not read.
    pub linked: Vec<JiraLinkedIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraCommentBody {
    pub markdown: String,
    /// Appended after the Markdown as one ADF code block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_block: Option<JiraCodeBlock>,
}

/// Typed queries; the adapter builds and escapes the JQL (Internal Logic 3). Callers never pass JQL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JiraQuery {
    /// Issues carrying `label` that are assigned to the signed-in person and not in the Done category,
    /// plus the issues named in `also_keys` whatever their assignee or status.
    HandoversFor {
        label: String,
        also_keys: Vec<String>,
    },
    /// The person's search text from **Add to this workspace > Jira issue...**.
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraDownload {
    pub bytes_written: u64,
    pub sha256: Sha256Hex,
}

/// Where the adapter reaches Jira. `Default` is production: `https://{site}` and
/// `https://api.atlassian.com`, with the spec's 30 s call timeout.
/// Only the `test-support` feature can set anything else.
#[derive(Debug, Clone, Default)]
pub struct JiraEndpoints {
    /// Replaces `https://{site}` (API-token base and `connect_with_token`).
    pub site_base_url: Option<String>,
    /// Replaces `https://api.atlassian.com` (OAuth gateway and `accessible-resources`).
    pub gateway_base_url: Option<String>,
    /// Replaces the 30 s `REQUEST_TIMEOUT` of a call (the attachment download keeps 120 s).
    pub request_timeout: Option<std::time::Duration>,
}

impl JiraAdapter {
    /// The 30 s `REQUEST_TIMEOUT` of a call.
    pub(crate) const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
}
