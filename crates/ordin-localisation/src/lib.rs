//! Localisation (Localisation.md): decides which locale the person sees, loads the Project Fluent
//! catalogues, formats every text the core composes with Unicode CLDR rules, and (ADR-109) keeps the
//! person's own wording and languages in a layer on top of the compiled-in catalogues.
//!
//! The crate reads no environment variable and no file at run time: the catalogues are compiled in
//! (`build.rs`), and the person layer lives in the Journal's SQLite database.
#![forbid(unsafe_code)]

mod catalogue;
mod check;
mod error;
mod fmt;
mod migrations;
mod person;
mod service;
mod store;
mod types;

// The shared test helpers name the crate by its package name, here and in `tests/`.
#[cfg(test)]
extern crate self as ordin_localisation;

#[cfg(test)]
mod tests;

/// Generated: one constant per `en-GB` message, SCREAMING_SNAKE_CASE of the kebab-case id, plus
/// `MESSAGE_IDS`, `SAFETY_IDS` and `MESSAGE_AREAS` (build.rs step A6).
///
/// An identifier absent from `en-GB` does not compile:
///
/// ```compile_fail
/// let _ = ordin_localisation::id::NO_SUCH_MESSAGE;
/// ```
///
/// Nor can a `MessageId` be made from text outside this crate:
///
/// ```compile_fail
/// let _ = ordin_localisation::MessageId::new("approval-card-send-button");
/// ```
pub mod id {
    include!(concat!(env!("OUT_DIR"), "/message_ids.rs"));
}

/// (message id, area) for every `en-GB` message, sorted by id; the area is the `en-GB` file stem.
pub use id::MESSAGE_AREAS;
/// Every `en-GB` message whose identifier begins `approval-`, and every `en-GB` message whose
/// attached comment holds a line exactly `safety`; sorted.
// spec: ordin-desktop/components/Localisation#LOC-LF-2
pub use id::SAFETY_IDS;

pub use catalogue::{bundled_locales, message_ids};
pub use check::check_entry;
pub use error::{LocalisationError, ALL_CODES};
pub use migrations::{migrations, MODULE_NAME, SQLITE_MIGRATIONS};
pub use service::{Localisation, LocalisationConfig, PreferenceStore};
pub use types::*;

/// The `component` field of every log line (Localisation.md *Observability*).
pub(crate) const COMPONENT: &str = "localisation";
