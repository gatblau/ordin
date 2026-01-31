//! Unit tests of the Ordin Host: one per Error Table row (X1), one per Gherkin scenario with the
//! scenario's name (X2), and the registry check (X4). Every dependency is a stand-in (`support`).

mod acceptance;
mod acceptance_amendments;
mod error_table;
mod facts_as_triples;
mod memory_settings;
#[cfg(feature = "minutes")]
mod minutes;
#[cfg(feature = "minutes")]
mod minutes_tools;
mod smoke;
mod support;
mod task_forms;
