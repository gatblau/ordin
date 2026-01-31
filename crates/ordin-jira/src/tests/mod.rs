//! Unit tests: X1 (`error_table`), X2 (`scenarios`), X4 (`registry`), the conversions, and the
//! transport rules, over a `wiremock` Jira Cloud stand-in on loopback and an Identity double. The
//! replies come from `tests/support/fixtures.rs`, written from the spec's own examples; no real
//! network is used.

#[path = "../../tests/support/mod.rs"]
mod support;

mod conversions;
mod error_table;
mod harness;
mod registry;
mod scenarios;
mod transport;
