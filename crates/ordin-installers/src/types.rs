//! The public value types (Installers.md *Public Interface*) and the constants of *Configuration*.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ordin_agent_model::ResolvedToolPolicy;
use ordin_types::domain::HarnessKind;
use ordin_types::time::Timestamp;
use ordin_types::AgentInstanceId;

use crate::error::InstallError;

/// False once C-60 or C-01 rules Copilot out; then `installer_for(CopilotCli)` is
/// `INST_HARNESS_UNSUPPORTED` (the HLD's fallback: Claude Code first).
pub const COPILOT_SUPPORTED: bool = true;

/// The installer `HarnessKind::CopilotCli` maps to (C-60): the vscode layout. Becomes `AgentsMd` if
/// C-60 finds Copilot loads only the agents-md layout.
pub(crate) const COPILOT_INSTALLER: InstallerKind = InstallerKind::Vscode;

/// `record_schema` of every install record this crate writes.
pub const RECORD_SCHEMA: &str = "ordin.install/v1";
/// The install record, relative to the instance's `meta/` folder.
pub const RECORD_FILE: &str = "install-record.json";
/// Backups of replaced or kept files, relative to `meta/`.
pub(crate) const BACKUPS_DIR: &str = "backups";
/// The copy of the bundle manifest and lock file that repair trusts, relative to `meta/`.
pub(crate) const BUNDLE_COPY_DIR: &str = "bundle";
/// `@`-import inlining depth (*assumed here*: Claude Code's documented import depth).
pub const MAX_IMPORT_DEPTH: usize = 5;
/// The most files a rendered tree may hold.
pub const MAX_INSTALLED_FILES: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallerKind {
    ClaudeCode,
    Vscode,
    AgentsMd,
}

impl InstallerKind {
    /// The kebab-case spelling, for the `installer` log field.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            InstallerKind::ClaudeCode => "claude-code",
            InstallerKind::Vscode => "vscode",
            InstallerKind::AgentsMd => "agents-md",
        }
    }
}

/// `ClaudeCode` → ClaudeCode; `CopilotCli` → Vscode (C-60), or INST_HARNESS_UNSUPPORTED while the
/// constant `COPILOT_SUPPORTED` is false (C-60/C-01 rule Copilot out).
// spec: ordin-desktop/components/Installers#SA-RECON-01
pub fn installer_for(harness: HarnessKind) -> Result<InstallerKind, InstallError> {
    installer_for_with(harness, COPILOT_SUPPORTED)
}

/// `installer_for` with the C-60/C-01 outcome given, so both outcomes are tested.
pub(crate) fn installer_for_with(
    harness: HarnessKind,
    copilot_supported: bool,
) -> Result<InstallerKind, InstallError> {
    match harness {
        HarnessKind::ClaudeCode => Ok(InstallerKind::ClaudeCode),
        HarnessKind::CopilotCli if copilot_supported => Ok(COPILOT_INSTALLER),
        HarnessKind::CopilotCli => Err(InstallError::HarnessUnsupported { harness }),
    }
}

/// Called by: Package Catalogue (*Add an agent*, *Change*, update acceptance, trial), which builds it
/// from the agent instance, the harness the person's profile selects, and `Bundle.plan.plan_hash`.
// spec: ordin-desktop/components/Installers#SA-RECON-03
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallRequest {
    pub agent_instance_id: AgentInstanceId,
    pub harness: HarnessKind,
    /// `sha256:<hex>` of a bundle the Resolver and Compiler built.
    pub plan_hash: String,
}

/// The launch paths of an installed agent, read from `record(id).launch` (SA-RECON-02). The paths
/// are always `<folder>/instructions.md` and `<folder>/plugin`; the adapters take them from here
/// rather than composing them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "harness", rename_all = "snake_case")]
pub enum HarnessLaunch {
    ClaudeCode {
        working_dir: PathBuf,
        /// `<folder>/instructions.md`, for `--append-system-prompt-file`.
        append_system_prompt_file: PathBuf,
        /// `<folder>/plugin`, for `--plugin-dir`.
        plugin_dir: PathBuf,
        plugin_name: String,
    },
    Copilot {
        working_dir: PathBuf,
    },
}

/// How each of the agent's commands is named in a session (R-94).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CommandName {
    pub command_id: String,
    pub session_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAction {
    Created,
    Replaced,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstalledFile {
    /// '/'-separated path relative to the agent folder.
    pub path: String,
    /// 64 hex.
    pub sha256: String,
    pub size: u64,
    pub action: FileAction,
    /// Digest of the file that was there before this install wrote it, when one was.
    pub pre_sha256: Option<String>,
    /// Path relative to `meta/` of the backup of that file.
    pub backup: Option<String>,
}

/// The install record, `meta/install-record.json`; the single source of the launch paths, the tool
/// policy, `reference_only_parts` and `needs_person_present`.
// spec: ordin-desktop/components/Installers#SA-RECON-02
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallRecord {
    /// "ordin.install/v1".
    pub record_schema: String,
    pub agent_instance_id: AgentInstanceId,
    pub installer: InstallerKind,
    pub harness: HarnessKind,
    pub plan_hash: String,
    pub previous_plan_hash: Option<String>,
    /// SHA-256 hex of the lock-file bytes written into the folder.
    pub lock_file_sha256: String,
    /// The resolved tool policy, for the Rules Engine (ADR-090 item 1).
    pub tool_policy: ResolvedToolPolicy,
    pub launch: HarnessLaunch,
    pub commands: Vec<CommandName>,
    /// True when the agent's control loop is `human-pulled`.
    pub needs_person_present: bool,
    /// Identifiers of the parts the installer could place only as reference files.
    pub reference_only_parts: Vec<String>,
    pub files: Vec<InstalledFile>,
    pub installed_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallOutcome {
    pub record: InstallRecord,
    /// For `update_in_session`: the running session must be resumed to see the change (C-64).
    pub restart_required: bool,
    /// True when `verify_and_repair` rebuilt the folder.
    pub repaired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum VerifyOutcome {
    Verified,
    /// Files whose digest differs from the record, files missing, or the lock file changed.
    Mismatch {
        changed: Vec<String>,
        missing: Vec<String>,
    },
}

/// Input of the Claude Code session form for a release bundle (Release Builder P-9, C-61 (b)).
#[derive(Debug, Clone)]
pub struct ClaudeSessionFormInput {
    /// The package name; the plugin name is derived from it as in step 3.
    pub agent_name: String,
    /// The package version, for `plugin.json`.
    pub version: String,
    /// The package description, for `plugin.json`; empty when the package has none.
    pub description: String,
    /// The compiled always-on instructions (`AGENTS.md` text); `@` imports are inlined against
    /// `bundle_dir` as in step 3.
    pub instructions: String,
    /// Skill slug → the skill folder's files ('/'-separated relative paths → bytes).
    pub skills: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    /// Command slug → the command file's Markdown bytes.
    pub commands: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderedTree {
    pub files: BTreeMap<String, Vec<u8>>,
    /// (instructions.md, plugin dir, plugin name).
    pub launch_paths: Option<(String, String, String)>,
    pub commands: Vec<CommandName>,
    pub reference_only_parts: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Scenario: Error — Copilot ruled out
    #[test]
    fn error_copilot_ruled_out() {
        // Given COPILOT_SUPPORTED is false; when installer_for is called with CopilotCli.
        let error = installer_for_with(HarnessKind::CopilotCli, false).unwrap_err();
        // Then it fails with INST_HARNESS_UNSUPPORTED.
        assert_eq!(
            error,
            InstallError::HarnessUnsupported {
                harness: HarnessKind::CopilotCli
            }
        );
        assert_eq!(error.code(), "INST_HARNESS_UNSUPPORTED");
        assert!(!error.retryable());
        // Claude Code is never ruled out.
        assert_eq!(
            installer_for_with(HarnessKind::ClaudeCode, false),
            Ok(InstallerKind::ClaudeCode)
        );
    }

    #[test]
    fn copilot_maps_to_the_vscode_installer_while_supported() {
        assert_eq!(
            installer_for(HarnessKind::CopilotCli),
            Ok(InstallerKind::Vscode)
        );
        assert_eq!(
            installer_for(HarnessKind::ClaudeCode),
            Ok(InstallerKind::ClaudeCode)
        );
    }

    #[test]
    fn installer_kinds_spell_kebab_case() {
        for kind in [
            InstallerKind::ClaudeCode,
            InstallerKind::Vscode,
            InstallerKind::AgentsMd,
        ] {
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::String(kind.as_str().to_owned())
            );
        }
    }
}
