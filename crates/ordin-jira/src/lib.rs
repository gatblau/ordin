//! ordin's typed client for the company's Jira Cloud site, reached with a credential the person
//! holds (JiraAdapter.md).
//!
//! It parses Jira addresses, finds assignable people, searches issues with queries it builds
//! itself, reads an issue with its comments, attachments, sub-tasks and links, downloads an
//! attachment, and makes the four writes a handover needs. It keeps no tables and asks no approval;
//! every write is an external action in the Journal with a caller-minted key.
#![forbid(unsafe_code)]

mod adapter;
mod address;
mod adf;
mod client;
mod error;
mod parse;
mod query;
#[cfg(test)]
mod tests;
mod types;

pub use error::{JiraError, ALL_CODES};
pub use types::{
    JiraAttachment, JiraCodeBlock, JiraComment, JiraCommentBody, JiraDeps, JiraDownload,
    JiraEndpoints, JiraIssueContent, JiraIssueRef, JiraIssueSummary, JiraLinkedIssue, JiraProject,
    JiraProjectRef, JiraQuery, JiraStatusCategory, JiraUser,
};

use std::sync::Arc;

/// The Jira Adapter. Constructed only by the desktop program; the ordin server builds none, and its
/// callers hold `Option<Arc<JiraAdapter>>`.
// spec: ordin-desktop/components/JiraAdapter#SA-JIRA-05
pub struct JiraAdapter {
    journal: Arc<ordin_journal::Journal>,
    transport: client::Transport,
}
