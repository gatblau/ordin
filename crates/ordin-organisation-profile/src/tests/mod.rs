//! The crate's behavioural tests. They live inside the crate (not under `tests/`) because they reach
//! the `wiremock` stand-in over plain HTTP on loopback, which only a test build admits
//! (`ALLOW_LOOPBACK_HTTP`), and because two *Error Table* rows are defects that only a direct call to
//! the store can provoke.

mod acceptance;
mod error_table;
mod logging;
mod support;
