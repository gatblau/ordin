//! Unit tests. Every lower crate is the real one over real SQLite on a temporary file; the ArtCraft
//! apps are fake programs each test writes, and the mirror is a `wiremock` server on loopback.

mod acceptance;
mod behaviour;
mod error_table;
mod security;
mod support;
