//! The Checker (Checker.md): ordin's fixed code that decides whether a part or an agent is fit to be
//! offered, tried, published or kept installed — "Checked by ordin".
//!
//! It runs the part checks on every part read and every draft (vocabulary first, then each part, then
//! the cross-checks between parts), and adds ordin's own: the organisation's trust policy over tool
//! bindings and hooks (owning the executable-part decisions), name collisions, coverage of each job by
//! a task, the wording rules, the format loss report, conflicts between parts that can be installed
//! together, the fixed-packs rule, the publish gate and each rehearsal expectation. Before a part
//! draft, withdrawal or vocabulary change is published it checks the whole repository with the change
//! and rebuilds every agent that uses the changed part. No check can be waived, and every finding
//! names its fix where fixed code knows one.
//!
//! Like the Resolver and Compiler, it is built without a part source; the program binds the Catalogue
//! Service with [`Checker::install_part_source`]. Its runs and decisions are its own tables in the
//! journal database, registered as module `checker` through [`migrations`]. The crate reads no
//! environment variable; the one profile key it reads is `agent_catalogue.trust_policy`.
#![forbid(unsafe_code)]

mod body;
mod changes;
mod checker;
mod context;
mod cross;
mod dossier;
mod draft;
mod error;
mod finding;
mod log;
mod model;
mod overlay;
mod rehearsal;
mod repo;
mod stage_one;
mod store;
mod trust;
mod wording;

#[cfg(test)]
mod tests;

use ordin_journal::{MigrationStep, ModuleMigrations};

pub use checker::{check_parts, Checker};
pub use error::{CheckError, ALL_CODES};
pub use model::*;

/// The module name the program registers the migrations under (DatabaseMigrations.md *Modules that
/// register migrations*).
pub const MODULE: &str = "checker";

const SQLITE_MIGRATIONS: &[MigrationStep] = &[(
    1,
    "create checker tables",
    include_str!("sql/sqlite_v1.sql"),
)];
const POSTGRES_MIGRATIONS: &[MigrationStep] = &[(
    1,
    "create checker tables",
    include_str!("sql/postgres_v1.sql"),
)];

/// `module = "checker"`, version 1 in each of `sqlite` and `postgres` (SA-RECON-05); the program
/// registers it with the Journal's migration registry.
// spec: ordin-desktop/components/Checker#SA-RECON-05
pub fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: MODULE,
        sqlite: SQLITE_MIGRATIONS,
        postgres: POSTGRES_MIGRATIONS,
    }
}
