//! The Installers (Installers.md): write a bundle into an agent folder for the session's harness —
//! Claude Code first, then Copilot — and keep the install record.
//!
//! Each installer renders the bundle into an in-memory file map (pure, golden-tree tested), and one
//! shared writer applies that map to the folder under a lease of the Agent Folder Manager,
//! recording every file's digest before and after so a later update or removal restores the folder
//! byte for byte. Under Claude Code's `--restricted` mode nothing in the agent folder is read
//! (C-61 (b)), so the Claude Code installer also writes `instructions.md` (the bootstrap with its
//! `@` imports inlined) and `plugin/` (skills and commands as a plugin named after the agent). An
//! installed folder is verified against its record before each session and rebuilt without asking
//! when it no longer matches (SA-INST-01). The tool policy is carried in the record for the Rules
//! Engine, never written for the harness to read.
//!
//! No environment variable, no profile key (Configuration: none); the constants are in
//! [`types`](crate::COPILOT_SUPPORTED). The crate has no Journal dependency: the caller journals.
#![forbid(unsafe_code)]

mod claude;
mod copilot;
mod error;
mod fsio;
mod imports;
mod installers;
mod log;
mod render;
mod types;

pub use error::{InstallError, ALL_CODES};
pub use installers::{render_claude_session_form, write_tree, Installers};
// spec: ordin-desktop/components/Installers#SA-RECON-05
pub use ordin_agent_model::{
    BundleManifest, LoadingMode, LockFile, PartFiles, PartKind, PartRef, ResolvedToolPolicy,
    SourceScope,
};
pub use ordin_resolver::Bundle;
pub use ordin_types::domain::HarnessKind;
pub use types::{
    installer_for, ClaudeSessionFormInput, CommandName, FileAction, HarnessLaunch, InstallOutcome,
    InstallRecord, InstallRequest, InstalledFile, InstallerKind, RenderedTree, VerifyOutcome,
    COPILOT_SUPPORTED, MAX_IMPORT_DEPTH, MAX_INSTALLED_FILES, RECORD_FILE, RECORD_SCHEMA,
};
