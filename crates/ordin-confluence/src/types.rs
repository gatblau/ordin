//! The public types of the adapter (ConfluenceAdapter.md *Public Interface*, *Injected settings*).

use std::sync::Arc;

use ordin_approval::ApprovalService;
use ordin_files::FileService;
use ordin_identity::IdentityService;
use ordin_journal::Journal;
use ordin_types::time::Timestamp;
use ordin_types::{ConfluenceLinkId, PublishedCopyId, SourceLinkId};
use serde::{Deserialize, Serialize};

/// Injected through a `tokio::sync::watch` channel; a site change clears the cached `cloudId`.
// spec: ordin-desktop/components/ConfluenceAdapter#D-4
#[derive(Debug, Clone)]
pub struct ConfluenceSettings {
    /// `confluence.site` from the organisation profile (2D-2), an `https` URL; None = Confluence off.
    pub site: Option<url::Url>,
}

/// Owned by this crate; implemented by the desktop program over the Craft Toolkit's `to_markdown` once
/// C-13 confirms WordCraft's headless Word → Markdown export. The program maps a `CraftError` to its
/// `code()` text.
// spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-08
#[async_trait::async_trait]
pub trait WordToMarkdown: Send + Sync {
    async fn convert(&self, docx: Vec<u8>) -> Result<String, String>;
}

pub struct ConfluenceDeps {
    /// The adapter's tables live in `journal.datastore()` (decision D4; no separate pool).
    pub journal: Arc<Journal>,
    pub identity: Arc<IdentityService>,
    pub approvals: Arc<ApprovalService>,
    pub files: Arc<FileService>,
    /// Built by the program with SA-HTTP-02's TLS and proxy settings.
    pub http: reqwest::Client,
    pub settings: tokio::sync::watch::Receiver<ConfluenceSettings>,
    pub word_to_markdown: Option<Arc<dyn WordToMarkdown>>,
}

/// The person a pasted API token belongs to, as `GET /wiki/rest/api/user/current` names them
/// (CONF-TOKEN-1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConfluenceUser {
    /// 1–128 characters.
    pub account_id: String,
    /// 1–255 characters.
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteVersion {
    /// The Confluence version number as text ("7"); the "version read" of A-25.
    pub tag: String,
    pub changed_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteItem {
    pub item: RemoteItemRef,
    pub title: String,
    /// Path in the working copy relative to the link root (Constants).
    pub path: String,
    pub has_children: bool,
    pub size_bytes: Option<u64>,
    pub version: RemoteVersion,
}

#[derive(Debug, Clone)]
pub struct FetchedItem {
    pub item: RemoteItemRef,
    pub path: String,
    pub bytes: Vec<u8>,
    pub version: RemoteVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginChange {
    pub source_link_id: SourceLinkId,
    pub origin_path: String,
    pub new_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    pub changed_at: Timestamp,
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Audience {
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub people: Option<u32>,
    pub everyone_who_can_open: bool,
    /// Sorted account and group ids with read access; empty under the C-42 fallback.
    pub principals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReadyItem {
    pub confluence_link_id: ConfluenceLinkId,
    pub kind: ReadyKind,
    /// Relative to the link's folder, as `confluence_folder_pages.folder_path` is.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_copy_id: Option<PublishedCopyId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadyKind {
    New,
    Renamed,
    Removed,
    /// A published file whose change waits for the person's card (amendment 2026-01-24).
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-03
    Changed,
}

/// Whether the person's own changes in a linked folder are pushed without a card; the Workspace
/// Service derives it for each sync (ConfluenceAdapter.md amendment 2026-01-24).
// spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-01
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticPublishing {
    On,
    Off,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LinkSyncReport {
    /// The person's own changes, pushed automatically.
    pub pushed: u32,
    /// Changes by Ordin or an agent, left to the Push Service.
    pub waiting_for_approval: u32,
    /// New, renamed, removed.
    pub ready_to_publish: u32,
    pub changed_in_confluence: u32,
    pub failed: u32,
}
