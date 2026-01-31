//! The Workspace Service (WorkspaceService.md): the person's workspaces, each a named piece of work
//! holding **source links** — local folders, git repository folders, git remotes, SharePoint and
//! Confluence locations and Jira issues — with a local working copy of what has been read from the
//! remote ones. It creates, renames, archives, moves to ordin's Bin, restores and deletes
//! workspaces; adds and removes places and links; reads SharePoint, Confluence and Jira links
//! through their adapters; records each folder grant of an agent instance or crew in a workspace;
//! keeps the Confluence links, the email links and the email, Telegram and Slack grants; tracks the
//! state of each working-copy item the Push Service sends back; and asks the adapters what changed
//! at the origins when a workspace opens, at start and every 15 minutes.
//!
//! The tables live in the Journal's SQLite datastore, registered as module `workspaces` through
//! [`migrations`]. Logs carry identifiers, kinds, codes and counts, never a workspace name, a path,
//! an address or a file name.
#![forbid(unsafe_code)]

mod confluence;
mod error;
mod grants;
mod items;
mod jira;
mod links;
mod log;
mod mail;
mod origin;
mod remote;
mod repos;
mod saved;
mod service;
mod store;
mod team;
mod types;

#[cfg(test)]
extern crate self as ordin_workspaces;

#[cfg(test)]
mod tests;

use std::time::Duration;

use ordin_journal::ModuleMigrations;

pub use error::{WorkspaceError, ALL_CODES};

/// The folder under `<documents>/ordin` that holds every remote link's working copy (WS-WC-1);
/// a workspace's own folder never takes this name.
pub const LINKED_COPIES_DIR: &str = "Linked copies";
pub use mail::check_imap_state;
pub use ordin_confluence::{AutomaticPublishing, ConfluenceLocation};
pub use ordin_files::{FileService, Place, PlaceKind};
pub use ordin_sharepoint::SharePointLocation;
pub use ordin_types::domain::email::{
    EmailAccess, EmailGrant, EmailLink, EmailLinkKind, EmailLinkState, ImapFolderState,
    SlackAccess, SlackGrant, TelegramAccess, TelegramGrant,
};
pub use ordin_types::domain::places::{
    ConfluenceLink, ConfluenceLinkState, FolderAccess, FolderGrant, Grantee, SourceLink,
    SourceLinkKind, SourceLinkState, WorkingCopyItem, WorkingCopyState, Workspace, WorkspacePlace,
    WorkspaceState,
};
pub use ordin_types::domain::runs::ShareHost;
pub use ordin_types::domain::server::ChannelKind;
pub use ordin_types::num::Sha256Hex;
pub use ordin_types::time::Timestamp;
pub use ordin_types::wire::workflow_file::AgentRepositoryRef;
pub use ordin_types::TeamGrantId;
pub use ordin_types::{
    AgentInstanceId, ApprovalId, ConfluenceLinkId, CrewCharterId, EmailGrantId, EmailLinkId,
    FolderGrantId, PlaceId, RunId, SlackGrantId, SourceLinkId, TelegramGrantId, WorkingCopyItemId,
    WorkspaceId,
};
pub use service::WorkspaceService;
pub use team::{
    check_result_path, GrantDecision, NewPendingGrant, OriginReach, PendingAccess, PendingGrant,
    PendingGrantState, Reach, TeamResultFile, TEAM_RESULTS_DIR_PREFIX, TEAM_RESULTS_NAME,
};
pub use types::{
    BinImpact, ConfluencePublishing, InboxOrigin, LinkLocation, LinkProgress, LinkedRepository,
    NewEmailLink, OpenReport, OriginChanged, OriginVersion, PlaceFileRef, WorkingCopyEntry,
    WorkspaceDeps, WorkspaceListFilter, WorkspaceSummary,
};

/// The module name under which the program registers this crate's migrations.
pub const MODULE_NAME: &str = "workspaces";

/// 15 minutes while ordin runs (A-36, A-37 analogue; B7 contract D-8).
pub const CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// 2 links at a time across workspaces.
pub const CHECK_CONCURRENCY: usize = 2;
/// 4 items at a time per link.
pub const FETCH_CONCURRENCY: usize = 4;
/// 100,000 files per link (A-21).
pub const LINK_MAX_ITEMS: usize = 100_000;
/// 20 folder levels per link (A-21).
pub const LINK_MAX_FOLDER_LEVELS: usize = 20;
/// 5 GiB of content per remote link; items beyond are listed as not read and read on first use
/// (*assumed here*; bounds disk use).
pub const LINK_READ_BUDGET: u64 = 5 * 1024 * 1024 * 1024;
/// 5 s after the last change in a linked folder before `sync_linked_folder`.
pub const LINKED_FOLDER_DEBOUNCE: Duration = Duration::from_secs(5);
/// 100 MiB per saved file (*chosen here*; above the chat channels' receive limits).
pub const SAVED_FILE_MAX_BYTES: u64 = 100 * 1024 * 1024;
/// 100 MiB per Jira attachment, the Jira Adapter's `MAX_ATTACHMENT_BYTES`; a larger one is listed as
/// not read.
pub const JIRA_ATTACHMENT_MAX_BYTES: u64 = 100 * 1024 * 1024;
/// How long `open` waits for the origin checks of the workspace before it returns (Internal Logic 7.1).
pub(crate) const OPEN_CHECK_WAIT: Duration = Duration::from_secs(10);

const SQLITE_MIGRATIONS: &[ordin_journal::MigrationStep] = &[
    (1, "workspace tables", include_str!("sql/sqlite_v1.sql")),
    (2, "slack grants", include_str!("sql/sqlite_v2.sql")),
    (3, "jira issue links", include_str!("sql/sqlite_v3.sql")),
    // Amended January 2026, ADR-121: the results folder, its files and the waiting grants.
    (4, "team workspaces", include_str!("sql/sqlite_v4.sql")),
];

/// Registered by the program with the Journal's `MigrationRegistry` (R3): module `workspaces`, four
/// SQLite steps, no PostgreSQL step (this module does not run in the agent profile).
// spec: ordin-desktop/components/WorkspaceService#SA-RECON-03
pub fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: MODULE_NAME,
        sqlite: SQLITE_MIGRATIONS,
        postgres: &[],
    }
}
