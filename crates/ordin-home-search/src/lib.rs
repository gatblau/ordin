//! Home and Search (HomeAndSearch.md, Playbook Step 58, with the ADR-106 amendment): **Home**, the
//! tree's changed marks, the **Search or go to…** box and **Run again**, for the person only, in
//! the application's own code with no model call. A read-side module: it keeps only its own marks
//! and builds every view from its dependencies' read methods and the search sources the program
//! registers.
#![forbid(unsafe_code)]

mod error;
mod ports;
mod service;
mod store;
mod types;

#[cfg(test)]
mod tests;

pub use error::{HomeError, ALL_CODES};
pub use ordin_journal::{MigrationStep, ModuleMigrations};
pub use ordin_types::domain::places::{ChangedMark, HomeMarks};
pub use service::{HomeSearch, HomeSearchDeps};
pub use types::{
    CopyUpdateSummary, FoundResult, GoToKind, GoToResult, HomeRow, HomeSourceError, HomeView,
    JournalReader, OpenTarget, RunAgainInput, RunAgainSubject, RunSummary, SearchAnswer,
    SearchSource, VersionCheck, WaitingItem,
};

/// The `component` field of every log line (*Observability*).
pub(crate) const COMPONENT: &str = "home";
