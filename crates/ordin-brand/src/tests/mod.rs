//! The crate's behavioural tests. They live inside the crate (not under `tests/`) because they reach
//! the `wiremock` asset host over plain HTTP on loopback, which only a test build admits
//! (`fetch::ALLOW_LOOPBACK_HTTP`), and because they drive Brand through its internal profile and
//! locale sources. Every test installs the process-wide log recorder before it reaches any
//! `tracing` call site (plan 01, FU-017).

mod acceptance;
mod error_table;
mod support;
