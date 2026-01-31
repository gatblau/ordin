//! Unit tests: one per Error Table row (X1), one per Gherkin scenario with the scenario's name (X2),
//! and the registry checks (X4). Log-capture tests are in `tests/logging.rs`, alone in their binary.

#[path = "../../tests/support/mod.rs"]
mod support;

mod acceptance;
mod build_script;
mod check_entry;
mod completeness;
mod error_sentences;
mod error_table;
mod person;
mod world;
