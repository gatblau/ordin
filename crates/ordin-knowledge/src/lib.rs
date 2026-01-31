//! The Knowledge Service (KnowledgeService.md): knowledge in two forms, one per program family.
//!
//! On the desktop and the personal server it keeps each workspace's Markdown pages in the
//! `knowledge/` folder of its own folder (an Open Knowledge Format bundle, ADR-112), with their
//! sources, freshness marks, links, names, tags and suggestions; answers the knowledge tools for
//! sessions through the MCP Front; gives the Memory Service each page's sections; shares pages with
//! the team and reads other teams' folders. In the agent profile it keeps a hosted agent's
//! knowledge items (chat history, entries, notes) with ranked full-text search on SQLite and
//! PostgreSQL, a port of wrightrun's KnowledgeIndex.
//!
//! It makes no model call and sends nothing outside the computer. Tables live in the Journal's
//! datastore, registered as module `knowledge` through [`KnowledgeService::migrations`]. Logs never
//! carry titles, bodies, queries, questions, item or note texts, sender names or locations.
#![forbid(unsafe_code)]

// The in-crate tests share the integration tests' fakes, which name this crate by its package name.
#[cfg(test)]
extern crate self as ordin_knowledge;

mod agent;
mod agent_pg;
mod agent_sqlite;
mod error;
mod fresh;
mod indexer;
mod ingest;
mod links;
mod markdown;
mod model;
mod names;
pub mod okf;
mod outside;
mod pagefile;
mod pages;
mod ports;
mod read;
mod reader;
mod search;
mod service;
mod store;
mod suggest;
mod team;
mod tools;

#[cfg(test)]
mod tests;

pub use error::{KnowledgeError, ALL_CODES};
pub use model::*;
pub use ports::*;
pub use service::{
    KnowledgeService, DESCRIPTION_MAX, INDEX_DEBOUNCE, MAX_LINKS_PER_PAGE, MAX_LINK_SUGGESTIONS,
    MAX_PAGES_PER_WORKSPACE, MAX_READ_PAGES_PER_FOLDER, MAX_TOOL_LINKS, MAX_VERIFIED, MODULE,
    RENAME_REWRITE_DELAY, REWRITE_ATTEMPTS,
};
