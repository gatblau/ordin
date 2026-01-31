//! The Agent Folder Manager (AgentFolderManager.md): creates, locks and removes agent folders,
//! one per agent instance under the application data root, and applies, for each session, the
//! folder grants the person made in the workspace the run started in.
//!
//! The agent folder is the harness session's working directory; ordin's own files for the instance
//! (the install record, backups, the lock file) live in `meta/` beside it, out of a session's
//! reach. The component keeps a working copy of each workspace's grant set as the File Service
//! publishes it and derives every session's grants from that copy, so a withdrawn grant reaches a
//! running session without a separate revocation call. It records no grant: the Workspace Service
//! is the one recorder.
//!
//! The crate exports no trait; consumers hold `Arc<AgentFolderManager>` and match [`AfmError`]. It
//! reads no environment variable: the program passes the application data root.
#![forbid(unsafe_code)]

mod db;
mod error;
mod grants;
mod lock;
mod log;
mod manager;
mod paths;
mod types;

#[cfg(test)]
mod tests;

pub use error::{AfmError, ALL_CODES};
pub use lock::{FolderLease, WRITE_LOCK_WAIT};
pub use manager::{AgentFolderManager, AGENTS_DIR};
pub use ordin_journal::{MigrationStep, ModuleMigrations};
pub use types::{
    AgentFolder, AgentFolderState, AppliedGrant, LockMode, RecoveryReport, SessionAccess,
    WorkspaceGrant,
};

/// The module name under which the program registers [`migrations`] with the Journal.
pub const MODULE: &str = "agent_folders";

/// Migration 1 creates `agent_folders`, `session_folder_access` and `workspace_folder_grants`
/// (Data Model). The program registers the result with the Journal's migration registry.
///
/// Rolling back is dropping the three tables, which forgets which folders exist; never run on a
/// populated database without a backup.
// spec: ordin-desktop/components/AgentFolderManager#SA-RECON-03
pub fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: MODULE,
        sqlite: &[(
            1,
            "create agent folder tables",
            include_str!("sql/sqlite_v1.sql"),
        )],
        postgres: &[(
            1,
            "create agent folder tables",
            include_str!("sql/postgres_v1.sql"),
        )],
    }
}
