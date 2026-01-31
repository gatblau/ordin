//! The in-crate tests: every Error Table row (X1) and every Gherkin scenario (X2) of the desktop
//! and personal-server half, each test named as its scenario. The agent-profile scenarios run
//! from `tests/agent_profile.rs` on SQLite and on PostgreSQL 18.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../../tests/support/fakes.rs"]
pub(crate) mod fakes;
#[path = "../../tests/support/logs.rs"]
pub(crate) mod logs;
pub(crate) mod rig;

mod errors;
mod links;
mod okf;
mod pages;
mod team;
