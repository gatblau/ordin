//! The unit and scenario tests of the crate, in-crate so they can reach the recall log's rows and
//! stand in for the Knowledge Query Client: one test per Error Table row (X1), one per Gherkin
//! scenario under the scenario's name (X2), and the registry check (X4).

mod acceptance;
mod agentic;
mod behaviour;
mod error_table;
mod held;
mod knowledge_agents;
#[path = "../../tests/support/models.rs"]
mod models;
mod models_fetched;
mod read_folders;
mod security;
mod support;
mod team;
