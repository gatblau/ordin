//! 2C part 1 § Engineering mode and Craft.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- Project ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Project {
    pub id: ProjectId,
    pub place_id: PlaceId,
    pub display_name: String,
    pub is_git_repository: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_branch: Option<String>,
    pub language_servers_in_use: Vec<String>,
    pub check_commands: Vec<String>,
    pub state: ProjectState,
    pub opened_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ProjectState {
    Opened,
    Indexed,
    Closed,
}

// ---- GithubOperationSetting ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct GithubOperationSetting {
    pub id: GithubOperationSettingId,
    pub project_id: ProjectId,
    /// `gh` operation kind; the catalogue of kinds is fixed in /spec (LD-33).
    pub operation_kind: String,
    pub is_write: bool,
    pub approval_required: bool,
    pub set_by: GithubSettingSource,
    pub set_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum GithubSettingSource {
    OrganisationProfile,
    Engineer,
}

// ---- LanguagePlugin ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct LanguagePlugin {
    pub id: LanguagePluginId,
    pub language: String,
    pub approved_servers: Vec<String>,
    pub default_server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chosen_server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_version: Option<String>,
    pub source: LanguagePluginSource,
    pub enabled: bool,
    /// The engineer's server settings (JSON object).
    pub server_settings: serde_json::Value,
    pub status: LanguagePluginStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LanguagePluginSource {
    AgentPackage,
    OrganisationProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LanguagePluginStatus {
    Available,
    Added,
    Installed,
    Removed,
}

// ---- CodingTask ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct CodingTask {
    pub id: CodingTaskId,
    pub origin: RunOrigin,
    pub project_id: ProjectId,
    pub agent_instance_id: AgentInstanceId,
    pub request: String,
    pub worktree_path: String,
    /// `ordin/<task name>`.
    pub branch: String,
    pub base_branch: String,
    pub base_commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_group_id: Option<AttemptGroupId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_outcome: Option<TestOutcome>,
    pub new_problems: u32,
    pub state: CodingTaskState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TestOutcome {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CodingTaskState {
    Working,
    ReadyForReview,
    Settled,
    SetAside,
}

// ---- ChangeSet ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ChangeSet {
    pub id: ChangeSetId,
    pub coding_task_id: CodingTaskId,
    pub run_id: RunId,
    pub project_id: ProjectId,
    pub entries: Vec<ChangeSetEntry>,
    pub state: ChangeSetState,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ChangeSetEntry {
    pub id: ChangeSetEntryId,
    pub file: String,
    /// Unified-diff hunk text.
    pub hunk: String,
    pub state: ChangeEntryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ChangeEntryState {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ChangeSetState {
    Open,
    InReview,
    Settled,
}

// ---- CraftInstallation ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct CraftInstallation {
    pub id: CraftInstallationId,
    pub app_name: String,
    pub pinned_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_result: Option<VerificationResult>,
    pub cli_modes: Vec<CraftCliMode>,
    pub status: CraftInstallationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VerificationResult {
    Verified,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CraftCliMode {
    Mcp,
    Run,
    Batch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CraftInstallationStatus {
    Approved,
    Downloaded,
    Installed,
    VerificationFailed,
    Removed,
}

// ---- FileTypeMapEntry ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct FileTypeMapEntry {
    pub id: FileTypeMapEntryId,
    pub extensions: Vec<String>,
    pub media_types: Vec<String>,
    /// App that opens it for the person; `None` = the system's default app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_with_app: Option<String>,
    /// Craft tools agents use to change it; empty = agents do not change it.
    pub craft_tools: Vec<String>,
    pub profile_version: String,
}
