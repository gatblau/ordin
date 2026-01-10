//! ordin's append-only record and the owner of its datastore (JournalAndArtefactStore.md).
//!
//! Three layers: a [`Datastore`] over a SQLite file or a PostgreSQL pool; the [`MigrationRegistry`]
//! every table-owning module feeds with versioned SQL (DatabaseMigrations.md lands here); and the
//! [`Journal`] service, which numbers and delivers Service API events, keeps artefacts, tool
//! requests, external actions, start-up reports, reports, dashboards, plans and internal entries,
//! and applies the person's history retention.
//!
//! The write vocabulary is `append_entry` (records) and `append_event` (events) only. The crate
//! reads no environment variable: every path and connection string is passed in by the program.
#![forbid(unsafe_code)]

mod codec;
mod datastore;
mod db;
mod error;
mod export;
mod journal;
mod migrate;
mod plans;
mod records;
mod reports;
mod retention;
mod types;
mod validate;

pub use datastore::{DatabaseUrl, Datastore, DatastoreConfig, Engine};
pub use error::{JournalError, ALL_CODES};
pub use journal::{Clock, Journal, JournalOptions, SystemClock};
pub use migrate::{
    ApplyMode, MigrationRegistry, MigrationReport, MigrationStep, ModuleMigrations, MIGRATIONS,
};
pub use types::{
    ArtefactFilter, DiagnosticEntry, EntryFilter, EventSink, JournalEntry, JournalEntryId,
    JournalEvent, NewArtefact, NewDashboard, NewExternalAction, NewJournalEntry, PlannedStep,
    PruneFailure, PruneParticipant, PruneRequest, PruneSummary, ReportSummary, RetentionSetting,
    RunIndexEntry, SubscriptionId, SupportBundle,
};
