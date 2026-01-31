//! The Package Inspector (PackageInspector.md): reads an installed agent without executing it and
//! derives its parsed package model, what it may do, the views of "What it's made of", "Show the
//! installed files" and "Read it", and the Ordin Host's agent directory for a workspace.
//!
//! A read-only module over the files the Installers and the Resolver and Compiler wrote. The model
//! is computed once per install or update and kept as one JSON file per instance; the crate has no
//! Journal dependency and so no table. Text it reads is data and none of it is obeyed.
#![forbid(unsafe_code)]

mod derive;
mod error;
mod fsio;
mod inspector;
mod may_do;
mod model;
mod places;
mod views;

pub use error::{InspectError, ALL_CODES};
pub use inspector::PackageInspector;
pub use may_do::{compare_may_do, may_do};
pub use model::{
    AgentDirectoryEntry, AuthoritySource, ConnectionEntry, Detail, InspectOutcome, InspectRequest,
    InstallFacts, InstalledFileEntry, InstalledFileText, InstalledFilesInput, InstalledMadeOf,
    KindGroup, MadeOfInput, MayDo, MayDoChange, MayDoChangeKind, ModelSource, NotCarried,
    NotCarriedReason, PackageModel, PartLine, PartPage, PartPageInput, PartState,
    RuleSetReloadFailure, TaskEntry, ToolEntry, ToolKind, MODEL_SCHEMA,
};
/// The Agent Model's task input types, carried by `TaskEntry.inputs` (PKI-TF-1), for the crates the
/// allowed graph keeps from the Agent Model.
pub use ordin_agent_model::{
    is_task_date, is_task_number, is_task_whole_number, TaskInput, TaskInputKind, DOCUMENTS_MIN,
    LONG_TEXT_MAX, SHORT_TEXT_MAX,
};
