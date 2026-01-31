//! The unit tests: X1 (one test per Error Table row), X2 (one test per Gherkin scenario, same name)
//! and the behaviour around them. The scenarios on git remotes are in `remotes.rs`, those on logging
//! in `tests/`.

#[path = "../../tests/support/mod.rs"]
pub(crate) mod support;

mod acceptance;
mod error_table;
// The host tests drive `gh` and `glab` stand-ins that are shell scripts, so they run on Unix only.
#[cfg(unix)]
mod hosts;
mod languages;
mod remotes;
#[cfg(unix)]
mod standin;
mod tools;
