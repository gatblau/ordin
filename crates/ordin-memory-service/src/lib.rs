//! The Memory Service (MemoryService.md): a memory of short facts that lasts across sessions, for
//! Ordin, the members of its crews, agent task sessions and, in the agent profile, the hosted agent.
//!
//! The crate is a thin owner around the memory component's transport-neutral service layer, linked
//! in-process with no network port and no identity provider (ADR-030, C-15). Every read and write
//! goes through that layer's authenticate, authorise, rate-limit, idempotency, component and audit
//! chain. The component's store lives in ordin's own datastore (the journal's SQLite file, or
//! PostgreSQL in the scaled shape) behind the storage port (ADR-101). The two bundled models run
//! in-process (ADR-033, SA-MEM-01).
//!
//! Tables `memory_*` and `mem_*` live in the Journal's datastore, registered as module
//! `memory_service` through [`MemoryService::migrations`]. No log line or journal record carries a
//! statement, an `about`, fact content, a query, origin text, a bearer, a thing name or a link kind.
#![forbid(unsafe_code)]

mod auth;
mod background;
mod config;
mod db;
mod error;
mod held;
mod knowledge;
mod model;
mod pages;
mod person;
mod provider;
mod providers;
mod recall;
mod remember;
mod service;
mod settings;
mod sql;
mod store;
mod team;
mod things;
mod topics;
mod triples;
mod views;
mod wider;

#[cfg(test)]
mod tests;

pub use config::{MemoryServiceConfig, ModelFileDigest, ModelManifest, ProgramKind};
pub use error::{MemoryError, ALL_CODES};
pub use memory::service::{ContentCheck, PersonalDataFinding};
#[doc(hidden)]
pub use memory::types::ports::{EmbeddingClient, ProviderError, RerankClient};
pub use model::*;
/// The `remember` schema's texts for a text value and a proposed word's label (MEM-TR-13).
pub use provider::{LINK_WORD as REMEMBER_LINK_WORD, TEXT_VALUE as REMEMBER_TEXT_VALUE};
pub use service::MemoryService;
#[doc(hidden)]
pub use service::TestSeams;
pub use topics::TOPIC_MAX_CHARS;
pub use triples::{SeedTerm, SEED_TERMS};

/// The module name under which the program registers this crate's migrations.
pub const MODULE_NAME: &str = "memory_service";
