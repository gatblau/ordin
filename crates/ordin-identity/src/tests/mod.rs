//! Unit tests: X1 (`error_table`), X2 (`scenarios`), X4 (`registry`), over a test harness that gives the
//! service an in-memory keychain or an encrypted file, a real SQLite file, and a `wiremock` stand-in on
//! loopback for every provider (no real network; the OS keychain is never touched).

mod error_table;
mod harness;
mod registry;
mod scenarios;
mod secrets;
