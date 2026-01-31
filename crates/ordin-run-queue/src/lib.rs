//! The Run Queue (RunQueue.md): the record of every run and thread of a hosted agent (agent
//! profile only), the lease an instance takes on a queued run, the pause and resume around an
//! approval, each run's event list, cancellation, and the counts and deletes the retention job and
//! an operator's erasure use.
//!
//! Three tables owned by this crate (`run_threads`, `run_records`, `run_events`) in the Journal's
//! datastore, registered as module `run_queue` through [`migrations`]. The storage trait
//! [`RunStore`] is implemented twice, [`SqliteRunStore`] and [`PgRunStore`]; [`DatastoreRunQueue`]
//! picks one from the datastore and adds validation, counters and log events. No environment
//! variable and no configuration key is read here.
#![forbid(unsafe_code)]

mod db;
mod error;
mod queue;
mod store;
mod types;
mod validate;

use ordin_journal::ModuleMigrations;

pub use error::{RunQueueError, ALL_CODES};
pub use queue::{DatastoreRunQueue, RunQueue};
pub use store::{
    ClosedRun, EndWrite, InsertFiring, InsertRun, PgRunStore, ProgressWrite, RunStore, Selection,
    SqliteRunStore,
};
pub use types::{
    CancelOutcome, DeletedRuns, Delivery, InProgressFilter, LeaseRenewal, MetricsSink, NewRun,
    PendingItem, PendingKind, RunEnd, RunEventKind, RunOutcome, RunPause, RunView, StoredRunEvent,
    TakenRun, LEASE_SECONDS, MAX_ARGUMENTS_BYTES, MAX_DELETE_LIMIT, MAX_EVENTS_PER_RUN,
    MAX_MESSAGE_CHARS, MAX_PROGRESS_LINE_CHARS, MAX_SESSION_RECORD_BYTES, MAX_STARTER_ID_CHARS,
};

/// This crate's migrations (module name `run_queue`; version 1 is the Data Model).
pub fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: "run_queue",
        sqlite: &[(1, "run queue", include_str!("sql/sqlite_v1.sql"))],
        postgres: &[(1, "run queue", include_str!("sql/postgres_v1.sql"))],
    }
}
