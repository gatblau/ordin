//! The Craft Toolkit (CraftToolkit.md): makes the ArtCraft apps available to people and agents. It
//! installs the apps the organisation profile approves at their pinned versions with no
//! administrator rights, opens a file in the matching app's window, serves each app's own MCP mode
//! through the MCP Front as the server `craft_<app>`, and makes the Word, PDF and slides copies of
//! a Markdown original.
//!
//! Tables `craft_installations`, `file_type_map_entries` and `file_type_extensions` live in the
//! Journal's datastore, registered as module `craft` through [`CraftToolkit::migrations`]. No log
//! line carries a tool argument value, a file path, file content or an app's result text.
#![forbid(unsafe_code)]

mod conversion;
mod definitions;
mod error;
mod filetypes;
mod install;
mod model;
mod opening;
mod profile;
mod provider;
mod servers;
mod store;
mod toolkit;

#[cfg(all(test, unix))]
mod tests;

pub use definitions::{
    AppWindow, CraftAppDefinition, CraftToolDefinition, CraftToolDefinitions, PathArg, PathRole,
    BUNDLED_CRAFT_TOOLS,
};
pub use error::{CraftError, ALL_CODES};
pub use model::{
    BlockedProgram, CraftAppPin, CraftDeps, CraftPinnedAsset, CraftPlatform, CraftProfile,
    CraftProgramCheck, MadeCopy, MakeCopy, OpenOutcome, ProgramCheckState, SyncReport,
};
pub use ordin_types::domain::engineering::{
    CraftCliMode, CraftInstallation, CraftInstallationStatus, FileTypeMapEntry, VerificationResult,
};
pub use ordin_types::domain::places::{CopyFormat, CopyMaker};
pub use provider::CraftAppsProvider;
pub use toolkit::CraftToolkit;
