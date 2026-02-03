//! The Setup Assistant (SetupAssistant.md, Playbook Step 59, with the ADR-114 releases-and-updates
//! amendment and the ADR-072 install-choice amendment): the guided "Getting you ready" sequence,
//! the checks repeated at start, in the background and on demand, the harness attachments,
//! **Send to IT**, the update check the person starts with background download and **What's new**,
//! the personal server's own update, and pinned language servers. The HealthChecks check runner
//! lands here (Playbook Step 59).
#![forbid(unsafe_code)]

mod assistant;
mod checks;
mod error;
mod language;
mod ports;
mod process;
mod store;
mod tools;
mod types;
mod updates;

#[cfg(test)]
mod tests;

pub use assistant::{bundled_language_servers, migrations, SetupAssistant};
pub use error::{SetupError, ALL_CODES};
pub use ordin_journal::{MigrationStep, ModuleMigrations};
pub use ordin_types::domain::identity::{HarnessAttachment, HarnessAttachmentState, HarnessKind};
pub use process::{find_claude_code, find_copilot_cli, ClaudeCodeProbe};
pub use tools::missing_path_tools;
pub use types::{
    AppUpdater, BundledProgram, CheckKind, CheckOutcome, CheckResult, CheckTrigger,
    HarnessLocation, HarnessProbe, HarnessSignIn, InstallKind, LanguageServerLaunch, MissingTool,
    PersonAction, ProbeError, ProbeRepair, ProgressStep, RefusedProgram, ServerPlatform,
    ServerRunMode, ServerUpdate, ServerUpdateOutcome, SetupConfig, SetupDeps, SetupProgram,
    SetupProgress, SetupReport, StepProblem, StepState, UpdateFinish, UpdateOffer, UpdateState,
    UpdaterError, UrlOpener, WhatsNew, SELF_UPDATE_UNAVAILABLE,
};

/// The `target` of every log line (*Observability*: target `ordin_setup`).
pub(crate) const TARGET: &str = "ordin_setup";
