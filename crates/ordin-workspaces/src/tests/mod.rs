//! Unit tests: X1 (`error_table`), X2 (`acceptance`, `jira_scenarios`), and the pure rules the
//! Error Table names (`rules`), over the real collaborators and `wiremock` stand-ins of
//! `tests/support/mod.rs`. X4 is `tests/registry.rs`.

#[path = "../../tests/support/mod.rs"]
mod support;

mod acceptance;
mod automatic_publishing;
mod error_table;
mod jira_scenarios;
mod read_paths;
mod rules;
