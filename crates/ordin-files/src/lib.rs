//! The File Service (FileService.md): the person's friendly access to the file system. It keeps
//! **places** (folders shown by friendly name, never by path), lists them in pages, previews the
//! common kinds, keeps a **version** of every file a run changes, imports dropped files, moves
//! removed files to the system's bin, **watches** places and keeps a local **index** of names and
//! document text that answers search across a workspace. It hands the per-workspace folder grants
//! to the Agent Folder Manager and the Rules Engine, records published copies and the person's held
//! edits, moves a file keeping its history (ADR-104), and, in the agent profile, keeps the hosted
//! agent's **work folder** in the datastore (ADR-098).
//!
//! The crate reaches its tables through the Journal's [`ordin_journal::Datastore`] (SA-RECON-01).
//! The file tables are SQLite only; the work folder runs on SQLite and PostgreSQL. It reads no
//! environment variable: the program passes the application data root. Logs carry `place_id` and
//! the extension only, never a path of the person's folders, file content or a search query.
#![forbid(unsafe_code)]

mod attribution;
mod copies;
mod documents;
mod error;
mod extract;
mod grants;
mod incoming;
mod index;
mod listing;
mod log;
mod moving;
mod paths;
mod preview;
mod search;
mod service;
mod versions;
mod watch;
mod workfolder;

#[cfg(test)]
mod tests;

use std::time::Duration;

pub use attribution::AgentWriteTarget;
pub use documents::{DocumentTools, DOC_MAX_TEXT_BYTES, DOC_PART_CHARS};
pub use error::{FileError, ALL_CODES};
pub use ordin_journal::{MigrationStep, ModuleMigrations};
pub use ordin_types::domain::places::{
    CopyDestination, CopyFormat, CopyMaker, CopyOriginal, FileVersion, FileVersionOrigin,
    FolderAccess, FolderGrant, Grantee, Place, PlaceKind, PublishedCopy, PublishedCopyState,
    PublishedVia,
};
pub use ordin_types::num::Sha256Hex;
pub use ordin_types::paging::{Cursor, Page, PAGE_SIZE};
pub use ordin_types::time::Timestamp;
pub use ordin_types::{
    AgentInstanceId, ConfluenceLinkId, FileVersionId, FolderGrantId, KnowledgePageId, PlaceId,
    PublishedCopyId, RunId, WorkspaceId,
};
pub use paths::media_type;
pub use service::{FileService, MODULE};
pub use types::{
    AddPlace, AgentRefusal, BinTicket, FileBytes, FileEntry, FileServiceDeps, FolderSort,
    GrepMatch, HeldEdit, IndexCoverage, PlaceChange, PlaceChangeKind, Preview, PruneReport,
    SavedFile, SearchHit, SearchResults, WorkFileEntry, WorkFolderConfig, WriteVersion,
};
pub use workfolder::{is_bundle_path, normalise_path, WorkFolder, MAX_FILE_BYTES};

mod types;

/// 100,000 files per place indexed; files beyond are listed but not indexed (A-21).
pub const MAX_PLACE_FILES: u64 = 100_000;
/// 20 levels indexed and watched (A-21).
pub const MAX_FOLDER_DEPTH: usize = 20;
/// 64 MiB for `read_file` and `read_version`; larger is refused.
pub const READ_LIMIT_BYTES: u64 = 64 * 1024 * 1024;
/// The first 256 KiB of a text or Markdown file.
pub const PREVIEW_TEXT_BYTES: usize = 256 * 1024;
/// The first 1 MiB of extracted text per file.
pub const INDEX_TEXT_BYTES: usize = 1024 * 1024;
/// Files above 50 MiB are indexed by name only.
pub const INDEX_MAX_SOURCE_BYTES: u64 = 50 * 1024 * 1024;
/// 500 ms per path.
pub const WATCH_DEBOUNCE: Duration = Duration::from_millis(500);
/// One line in the search dialog.
pub const SNIPPET_CHARS: usize = 160;
/// WR-FileTree `MAX_FILE_BYTES`.
pub const WF_MAX_FILE_BYTES: usize = 10_485_760;
/// WR-FileTree.
pub const WF_MAX_PATH_CHARS: usize = 1_024;
/// WR-FileTree.
pub const WF_BUNDLE_DIR_PREFIXES: [&str; 4] = ["/.apm/", "/knowledge/", "/acceptance/", "/brand/"];
/// WR-FileTree; ADR-098 keeps the declaration's file name.
pub const WF_BUNDLE_FILES: [&str; 3] = [
    "/AGENTS.md",
    "/wrightrun-manifest.json",
    "/wrightrun-declaration.yaml",
];
/// Extensions the incoming-file safety check refuses (Internal Logic 11.2).
pub const INCOMING_REFUSED_EXTENSIONS: [&str; 26] = [
    "exe", "msi", "bat", "cmd", "com", "scr", "pif", "ps1", "vbs", "vbe", "js", "jse", "wsf",
    "jar", "app", "dmg", "pkg", "sh", "command", "dll", "so", "dylib", "lnk", "docm", "xlsm",
    "pptm",
];

/// A path argument is at most this many bytes (Internal Logic 4.1).
pub(crate) const PATH_MAX_BYTES: usize = 4096;
/// The broadcast channel holds this many changes (Internal Logic 5.7).
pub(crate) const CHANGE_CHANNEL: usize = 4096;
/// How long a write this service made is remembered (Internal Logic 5.6).
pub(crate) const RECENT_WINDOW: Duration = Duration::from_secs(10);
