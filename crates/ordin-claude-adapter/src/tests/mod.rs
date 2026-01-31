//! The crate's own tests. `support` holds the fake `claude` and the test doubles of the spec's
//! *Background*; `error_table` has one test per Error Table row (X1); `acceptance` one test per
//! Gherkin scenario (X2); `behaviour` the rest of *Internal Logic*; `units` the pure helpers.

mod acceptance;
mod behaviour;
mod error_table;
mod support;
mod units;
