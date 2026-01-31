//! The public value types of the Craft Toolkit (CraftToolkit.md *Public Interface* and *Shared
//! Context*).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use ordin_files::FileService;
use ordin_mcp_front::ToolRegistry;
use ordin_organisation_profile::OrganisationProfileService;
use ordin_rules::RulesEngine;
use ordin_types::domain::engineering::CraftInstallation;
use ordin_types::domain::places::{CopyFormat, CopyMaker};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use serde::{Deserialize, Serialize};

use crate::definitions::CraftToolDefinitions;

pub struct CraftDeps {
    pub journal: ordin_journal::Journal,
    /// For `tools_changed`; registration is the program's (SA-RECON-01).
    pub registry: Arc<dyn ToolRegistry>,
    pub rules: Arc<RulesEngine>,
    pub files: Arc<FileService>,
    pub profile: Arc<OrganisationProfileService>,
    /// SA-HTTP-02 (system proxy, trust store, A-39).
    pub http: reqwest::Client,
    pub data_root: PathBuf,
    pub platform: CraftPlatform,
    /// Parsed from the bundled craft-tools.json at build time.
    pub definitions: CraftToolDefinitions,
}

/// Keys `macos_arm64`, `macos_x86_64`, `windows_x86_64`, `linux_x86_64` (K9); the program picks the
/// one matching the build target's operating system and architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftPlatform {
    MacosArm64,
    #[serde(rename = "macos_x86_64")]
    MacosX86_64,
    #[serde(rename = "windows_x86_64")]
    WindowsX86_64,
    #[serde(rename = "linux_x86_64")]
    LinuxX86_64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenOutcome {
    /// Opened in the mapped ArtCraft app's window.
    OpenedInApp { app: String },
    /// No ArtCraft app is mapped or installed: the screens ask
    /// "No editor for this kind of file — open it in your usual app?" (11 §6).
    NoEditor,
    /// Opened with the system's default app.
    OpenedWithSystemDefault,
}

pub struct MakeCopy {
    /// "rates.md"
    pub original_name: String,
    /// "supplier-docs" (repository) or the place's friendly name.
    pub original_from: String,
    /// "main" for a repository original.
    pub branch: Option<String>,
    /// The original at the version the Push Service names.
    pub markdown: String,
    /// Word, Pdf or Slides.
    pub format: CopyFormat,
    pub updated_at: Timestamp,
}

pub struct MadeCopy {
    pub bytes: Vec<u8>,
    pub file_name: String,
    pub maker: CopyMaker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncReport {
    pub installations: Vec<CraftInstallation>,
    /// Programs the operating system refused to start, for "Send to IT" (ADR-072, UX-A46).
    pub blocked: Vec<BlockedProgram>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BlockedProgram {
    pub program: String,
    pub publisher: Option<String>,
    pub sha256: Sha256Hex,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CraftProgramCheck {
    pub app_name: String,
    pub version: String,
    pub state: ProgramCheckState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgramCheckState {
    /// Installed at the pinned version, hash verified, and the start check passed.
    Ready,
    /// Was not ready and `repair` made it ready.
    Repaired,
    /// Not installed at the pinned version (download, hash or platform failure; `repair` false or
    /// failed).
    Missing,
    /// The operating system refused to start it (ADR-072, `BlockedProgram`).
    Refused {
        publisher: String,
        sha256: Sha256Hex,
    },
}

/// This crate's view of `CraftSection`, built at each snapshot.
#[derive(Debug, Clone)]
pub struct CraftProfile {
    pub profile_version: String,
    pub apps: Vec<CraftAppPin>,
    pub file_types: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CraftAppPin {
    pub name: String,
    pub version: String,
    pub platforms: BTreeMap<CraftPlatform, CraftPinnedAsset>,
    /// `None` = every tool the app's MCP mode lists (K9).
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct CraftPinnedAsset {
    pub asset_url: url::Url,
    pub file_name: String,
    pub sha256: Sha256Hex,
}
