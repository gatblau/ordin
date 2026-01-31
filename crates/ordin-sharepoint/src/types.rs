//! The public value types of the adapter (SharePointAdapter.md *Public Interface*).

use ordin_types::domain::runs::ExternalActionKind;
use ordin_types::time::Timestamp;
use ordin_types::{ExternalActionKey, RunId, SourceLinkId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SharePointItemRef {
    pub drive_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SharePointItem {
    pub item: SharePointItemRef,
    pub name: String,
    /// Relative to the listed location, `/`-separated; folders end with `/`.
    pub path: String,
    pub is_folder: bool,
    pub size_bytes: u64,
    /// `None` for folders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<SharePointVersion>,
}

#[derive(Debug, Clone)]
pub struct FetchedItem {
    pub item: SharePointItemRef,
    pub bytes: Vec<u8>,
    pub version: SharePointVersion,
}

/// Every write's external-action bracket (G8). The caller mints `key` (UUIDv7) and records nothing in the
/// Journal for it: this adapter begins the external action before sending and confirms it after (single
/// bracket, reconciliation rows 711 and 723). `kind` is one of `OriginPush`, `CopyPublish`,
/// `HandoverWritten`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalWrite {
    pub key: ExternalActionKey,
    pub run_id: Option<RunId>,
    pub kind: ExternalActionKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginChange {
    pub source_link_id: SourceLinkId,
    /// Relative to the link's root.
    pub origin_path: String,
    /// The new cTag; empty when removed.
    pub new_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    pub changed_at: Timestamp,
    pub removed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkScope {
    Users,
    Organization,
    Anonymous,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Audience {
    /// One line for the card: "Procurement team and 3 people", "Everyone in the company",
    /// "Anyone with the link", or "everyone who can open it" (C-42 fallback).
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub people: Option<u32>,
    pub everyone_who_can_open: bool,
    /// Sorted Graph principal ids (users, groups, site groups) with access; empty under the fallback.
    pub principals: Vec<String>,
    /// The broadest sharing-link scope on the item, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_scope: Option<LinkScope>,
}
