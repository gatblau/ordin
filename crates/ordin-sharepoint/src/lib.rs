//! The SharePoint Adapter (SharePointAdapter.md): reads from and pushes to SharePoint through Microsoft
//! Graph with the person's own Microsoft account. It lists the sites, libraries and folders the person can
//! see, fetches files with the version read, finds what changed at a linked location, uploads a new
//! version only when the origin still has the version that was read, creates files and folders, removes a
//! published copy to the recycle bin, and reports who can see a destination.
//!
//! The crate returns bytes and version tags to its callers; the Workspace Service writes the working copy
//! and the Push Service decides what to push. Its three tables live in the Journal's SQLite datastore
//! ([`migrations`]), and every write is one external-action bracket in the Journal.
//!
//! Conditional assertions relied on (SharePointAdapter.md *Confirmation item bound here*): C-14 (Identity
//! returns a token, else `SP_NOT_CONFIGURED`), C-42 (permissions read as one line, else the fallback
//! audience) and C-69 (the conditional write and the reads a handover needs; no directory search).
#![forbid(unsafe_code)]

mod adapter;
mod audience;
mod browse;
mod content;
mod error;
mod graph;
mod store;
mod track;
mod types;
mod wire;
mod write;

pub use adapter::{SharePointAdapter, SharePointDeps};
pub use error::{SharePointError, ALL_CODES};
pub use graph::SCOPES;
pub use types::{
    Audience, ExternalWrite, FetchedItem, LinkScope, OriginChange, SharePointItem,
    SharePointItemRef, SharePointLocation, SharePointVersion,
};

use ordin_journal::ModuleMigrations;

/// The module name under which the program registers this crate's migrations.
pub const MODULE_NAME: &str = "sharepoint";

/// Migrations registered by the program with the Journal's `MigrationRegistry`: module `sharepoint`,
/// one SQLite step (the Data Model SQL), no PostgreSQL step (the adapter runs on no agent-profile server).
pub fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: MODULE_NAME,
        sqlite: &[(1, "sharepoint tables", include_str!("sql/sqlite_v1.sql"))],
        postgres: &[],
    }
}
