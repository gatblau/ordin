//! The public types of the File Service that are not 2C's (FileService.md *Public Interface*).
//! 2C's own types (`Place`, `FileVersion`, `PublishedCopy` and their parts) are re-exported from
//! `ordin_types::domain::places` unchanged.

use std::path::PathBuf;
use std::sync::Arc;

use ordin_types::domain::places::{FileVersionOrigin, PlaceKind};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::{FileVersionId, PlaceId};
use serde::{Deserialize, Serialize};

/// What the program hands over at construction (Construction and registration).
pub struct FileServiceDeps {
    /// The datastore is `journal.datastore()` (SA-RECON-01).
    pub journal: Arc<ordin_journal::Journal>,
    pub rules: Arc<ordin_rules::RulesEngine>,
    pub agent_folders: Arc<ordin_agent_folders::AgentFolderManager>,
    /// The application data root (ADR-072); versions live under `<root>/versions/`.
    pub data_root: PathBuf,
}

pub struct AddPlace {
    pub path: PathBuf,
    pub friendly_name: Option<String>,
    pub kind: PlaceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderSort {
    NameAsc,
    NameDesc,
    KindAsc,
    ChangedDesc,
    ChangedAsc,
    ChangedByAsc,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct FileEntry {
    pub place_id: PlaceId,
    /// Relative to the place, `/`-separated; a folder ends with `/`.
    pub path: String,
    pub name: String,
    pub is_folder: bool,
    /// Lower-case extension without the dot; empty for a folder or no extension.
    pub kind: String,
    pub size_bytes: u64,
    pub changed_at: Timestamp,
    /// "Ordin", an agent instance's display name supplied by the writer, "you", or the
    /// origin's author recorded by `set_attribution`; absent when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    pub has_versions: bool,
}

#[derive(Debug, Clone)]
pub struct FileBytes {
    pub place_id: PlaceId,
    pub path: String,
    pub bytes: Vec<u8>,
    pub hash: Sha256Hex,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Preview {
    /// The WebView draws it from the bytes (png, jpeg, gif, webp, svg as an image, never as markup).
    Image {
        media_type: String,
        bytes_b64: String,
    },
    /// The WebView's own PDF viewer draws it.
    Pdf { bytes_b64: String },
    /// Plain text or Markdown, the first PREVIEW_TEXT_BYTES; `truncated` when longer.
    Text {
        text: String,
        markdown: bool,
        truncated: bool,
    },
    /// A platform thumbnail (macOS Quick Look); PNG.
    Thumbnail { png_b64: String },
    /// No preview for this kind on this platform; the screens show the kind's icon.
    None,
}

pub struct WriteVersion {
    pub place_id: PlaceId,
    pub path: String,
    pub bytes: Vec<u8>,
    pub origin: FileVersionOrigin,
    /// When present, the write is refused if the file's current hash differs (optimistic check).
    pub expected_hash: Option<Sha256Hex>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PruneReport {
    pub versions_removed: u64,
    pub bytes_freed: u64,
}

/// Undo record of one `move_to_bin`; the bytes are the version `file_version_id` in the version store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BinTicket {
    pub place_id: PlaceId,
    pub path: String,
    pub file_version_id: FileVersionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct PlaceChange {
    pub place_id: PlaceId,
    pub path: String,
    pub change: PlaceChangeKind,
    pub observed_at: Timestamp,
    /// True when the change is a write this service made for a run, a Craft output or a restore;
    /// false for the person's own edits, in ordin or in another app.
    pub by_ordin: bool,
    /// The version recorded for this change; absent when the path has no version history and none
    /// was written. Added for the Knowledge Service (reconciliation row 362).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// The origin of that version; absent exactly when `version` is absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<FileVersionOrigin>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaceChangeKind {
    Created,
    Modified,
    Removed,
    Renamed { from: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub coverage: IndexCoverage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchHit {
    pub place_id: PlaceId,
    pub path: String,
    pub name_match: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    pub changed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IndexCoverage {
    pub indexed: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct HeldEdit {
    pub place_id: PlaceId,
    pub path: String,
    pub file_version_id: FileVersionId,
    pub saved_at: Timestamp,
}

/// Result of `save_incoming`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SavedFile {
    pub place_id: PlaceId,
    /// Relative to the place; the file name, numbered " (2)", " (3)" … when the name is taken.
    pub path: String,
    /// True when the safety check found hidden text (Internal Logic 11.3); the file is saved and the
    /// caller reports it as for documents (02:404).
    pub hidden_content_found: bool,
}

/// What the Agent Loader's file tools call (Work folder).
pub struct WorkFolderConfig {
    /// `^[A-Za-z0-9_@+:~-]{1,64}$`; the agent configuration's work namespace, else the package name.
    pub namespace: String,
    /// The obtained release's bundle folder on local disk (read-only).
    pub bundle_dir: PathBuf,
    /// None: every work-folder path writable; Some(empty): none writable (WR-FileTree step 4).
    pub writable_paths: Option<Vec<String>>,
    /// `<duty folder>STOP` and `<duty folder>escalation.md` of each declared duty.
    pub protected_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct WorkFileEntry {
    pub path: String,
    pub size_bytes: u64,
    pub modified_at: Timestamp,
    pub bundle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct GrepMatch {
    pub path: String,
    pub line: u32,
    pub text: String,
}

/// A refusal whose text is given to the model verbatim (becomes `FILE_AGENT_REFUSED`).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct AgentRefusal(pub String);
