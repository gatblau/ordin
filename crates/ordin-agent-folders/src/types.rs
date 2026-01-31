//! The public value types of the Agent Folder Manager (AgentFolderManager.md *Public Interface*).

use std::path::PathBuf;

use ordin_types::domain::places::{FolderAccess, FolderGrant, PlaceKind};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, PlaceId, RunId, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentFolder {
    pub agent_instance_id: AgentInstanceId,
    /// `<data root>/agents/<id>/folder` — the harness working directory; 2C `AgentInstance.agent_folder_path`.
    pub folder_path: PathBuf,
    /// `<data root>/agents/<id>/meta` — ordin's own files for the instance; never given to a session.
    pub meta_path: PathBuf,
    pub state: AgentFolderState,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentFolderState {
    Created,
    Removing,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// A harness session uses the folder; many at once.
    Session,
    /// Install, update, repair or removal; excludes every other lease.
    Write,
    /// The pack-choice rebuild during a session (ADR-089 decision 64, C-64): allowed beside
    /// `Session` leases, excludes `Write` and another `SessionUpdate`.
    SessionUpdate,
}

impl LockMode {
    /// The value of the `requested_mode` log field.
    pub(crate) fn label(self) -> &'static str {
        match self {
            LockMode::Session => "session",
            LockMode::Write => "write",
            LockMode::SessionUpdate => "session_update",
        }
    }
}

/// One grant of a workspace as the File Service publishes it (B7 contract D-7): the 2C grant plus
/// the place's absolute path and kind, read by the File Service from its `places` row, never from a
/// tool argument.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceGrant {
    pub grant: FolderGrant,
    pub place_path: PathBuf,
    pub place_kind: PlaceKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AppliedGrant {
    pub place_id: PlaceId,
    pub path: PathBuf,
    pub access: FolderAccess,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionAccess {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub agent_folder: PathBuf,
    pub grants: Vec<AppliedGrant>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RecoveryReport {
    pub removals_finished: u32,
    pub sessions_cleared: u32,
    pub unknown_directories: u32,
}
