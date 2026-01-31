//! The Team Knowledge Service (TeamKnowledgeService.md): a workspace's own team folder in a GitHub
//! or GitLab repository — **Share what we've learned** and **Get them** of facts and knowledge pages
//! (ADR-103, format 2 of ADR-118) — and the team workspace of ADR-121: the workspace description,
//! **Share as a team workspace…**, **Open a team workspace…**, waiting grants, activity, results and
//! **Leave the team workspace**.
//!
//! This is the first build (ADR-121 item 9): reading other teams' folders (ADR-110) and the index and
//! log of ADR-112 are not built; their migration steps are applied with their tables left empty.
//!
//! Tables live in the Journal's datastore, module `team_knowledge` ([`TeamKnowledgeService::migrations`]).
#![forbid(unsafe_code)]

mod error;
mod files;
mod git;
mod model;
mod ports;
mod service;
mod sink;
mod store;
mod sync;
mod team_ws;
mod text;

#[cfg(test)]
mod tests;

pub use error::{
    TeamError, ALL_CODES, TEAM_WS_AGENT_NEEDS_IT, TEAM_WS_AGENT_PACK_MISSING, TEAM_WS_AGENT_UNSAFE,
};
pub use files::{
    TeamAccessEntry, TeamActivityFile, TeamAgentEntry, TeamPlaceEntry, TeamResultsEntry,
    TeamWorkspaceFile, LOGIN_PATTERN, TEAM_CHANGES, TEAM_RESULT_MAX_BYTES,
};
pub use model::*;
pub use ordin_memory_service::{
    TeamEntity, TeamExportPage, TeamFact, TeamImportBatch, TeamImportOutcome, TeamImportResult,
    TeamRelationship, TeamSource, TeamTerm, TeamTriple,
};
pub use ports::*;
pub use service::{TeamKnowledgeService, CHECK_INTERVAL, MODULE};
