//! Unit tests: X1 (`error_table`), X2 (`scenarios`), X4 (`registry`), and the behaviour the
//! scenarios do not reach (`behaviour`, `merge`), over a `wiremock` stand-in for the knowledge route
//! group on loopback and an Identity double. No real network is used.

mod behaviour;
mod error_table;
mod harness;
mod merge;
mod registry;
mod scenarios;
