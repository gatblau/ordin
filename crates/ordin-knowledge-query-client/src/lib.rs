//! The asking side of knowledge agents (KnowledgeQueryClient.md, ADR-113 and ADR-115).
//!
//! On the desktop and the personal server the crate keeps each workspace's front doors (the
//! knowledge agents a person connected, or the organisation profile named) and sends a recall
//! question to all of them in parallel, within a deadline, merging what arrives into one ranked list.
//! In the agent profile the same code is the front door's way of asking its proxies, with a backup
//! request, a hop budget and a list of agents already visited. It sends the question and its filters
//! only, never conversation text, workspace facts or file contents, and answers no query itself.
#![forbid(unsafe_code)]

mod accounts;
mod ask;
mod client;
mod error;
mod merge;
mod store;
mod streams;
#[cfg(test)]
mod tests;
mod tokens;
mod transport;
mod types;

pub use client::KnowledgeQueryClient;
pub use error::{KqError, ALL_CODES};
pub use merge::merge_hits;
pub use types::*;

/// The module name under which the program registers this crate's migrations.
pub const MODULE_NAME: &str = "knowledge_query_client";

/// The tables of this module: `kq_connections` and `kq_connection_contents` (version 1) and
/// `kq_connection_accounts` (version 2, ADR-115). SQLite on D, P and the single shape; the
/// PostgreSQL slice creates the same tables, which hold no rows (the agent profile has no workspaces).
pub fn migrations() -> ordin_journal::ModuleMigrations {
    ordin_journal::ModuleMigrations {
        module: MODULE_NAME,
        sqlite: &[
            (
                1,
                "create knowledge connection tables",
                include_str!("sql/sqlite_v1.sql"),
            ),
            (
                2,
                "create knowledge connection accounts",
                include_str!("sql/sqlite_v2.sql"),
            ),
        ],
        postgres: &[
            (
                1,
                "create knowledge connection tables",
                include_str!("sql/postgres_v1.sql"),
            ),
            (
                2,
                "create knowledge connection accounts",
                include_str!("sql/postgres_v2.sql"),
            ),
        ],
    }
}
