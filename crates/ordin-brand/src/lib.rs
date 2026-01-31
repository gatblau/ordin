//! Brand (Brand.md): applies the run layer of the brand pack (ADR-041) — product name,
//! coordinator name, logos, icon set, design tokens, fonts, links, About and legal text — that the
//! desktop shows inside its window.
//!
//! It reads the brand section of the active organisation profile, downloads and verifies the pack's
//! assets, refuses a pack whose colour tokens fail WCAG 2.2 AA contrast or that tries to set a colour
//! the application keeps fixed, and falls back to the build's own pack (compiled in from
//! `build-brand/`) while keeping the refusal reason for IT. The table `brand_packs` lives in the
//! Journal's datastore, registered as module `brand` through [`migrations`].
#![forbid(unsafe_code)]

mod check;
mod error;
mod fetch;
mod model;
mod service;
mod store;

#[cfg(test)]
mod tests;

/// The build pack compiled in by build.rs: `BRAND_JSON` and `ASSET_FILES`.
mod build_files {
    include!(concat!(env!("OUT_DIR"), "/build_brand.rs"));
}

pub use check::RefusalKind;
pub use error::{BrandError, ALL_CODES};
pub use model::{AssetBytes, AssetRef, BrandConfig};
pub use service::Brand;

/// The `component` field of every log line (Brand.md *Observability*).
pub(crate) const COMPONENT: &str = "brand";

/// The module name under which the program registers [`migrations`].
const MODULE_NAME: &str = "brand";

/// *Data Model*, version 1.
const MIGRATION_0001: &str = include_str!("sql/brand_v1.sql");

/// `{ module: "brand", sqlite: [(1, "create brand_packs", MIGRATION_0001)], postgres: [] }`. No
/// PostgreSQL variant: Brand runs on the desktop only.
// spec: ordin-desktop/components/Brand#SA-RECON-01
pub fn migrations() -> ordin_journal::ModuleMigrations {
    ordin_journal::ModuleMigrations {
        module: MODULE_NAME,
        sqlite: &[(1, "create brand_packs", MIGRATION_0001)],
        postgres: &[],
    }
}
