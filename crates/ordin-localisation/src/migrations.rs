//! Migrations for the program to register with the Journal's `MigrationRegistry`
//! (Localisation.md *Data Model*; DatabaseMigrations.md).

use ordin_journal::{MigrationStep, ModuleMigrations};

/// The module name under which the program registers [`SQLITE_MIGRATIONS`].
// spec: ordin-desktop/components/Localisation#SA-RECON-01
pub const MODULE_NAME: &str = "localisation";

const MIGRATION_0001: &str = include_str!("sql/localisation_v1.sql");
const MIGRATION_0002: &str = include_str!("sql/localisation_v2.sql");

/// `(version, description, SQL)`. Step 1 creates `locale_preferences`; step 2 (ADR-109) creates the
/// person layer. Forward-only (SA-XC-07).
pub const SQLITE_MIGRATIONS: &[MigrationStep] = &[
    (1, "create locale_preferences", MIGRATION_0001),
    (2, "create person language tables", MIGRATION_0002),
];

/// `{ module: "localisation", sqlite: [steps 1 and 2], postgres: [] }`. No PostgreSQL variant: the
/// agent profile's scaled shape holds no preference.
pub fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: MODULE_NAME,
        sqlite: SQLITE_MIGRATIONS,
        postgres: &[],
    }
}
