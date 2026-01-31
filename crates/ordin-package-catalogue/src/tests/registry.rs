//! X4: every code in `ALL_CODES` is registered, and the prefixes are owned by this crate.

use ordin_types::error_registry::{lookup, owns_prefix};

use crate::ALL_CODES;

const CRATE: &str = "ordin-package-catalogue";

#[test]
fn every_code_is_found_by_the_registry_and_its_prefix_is_owned() {
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, CRATE, "{code}");
        let prefix = code.split('_').next().expect("a prefix");
        assert!(
            owns_prefix(CRATE, prefix),
            "{prefix} is not owned by {CRATE}"
        );
    }
}

#[test]
fn every_code_of_the_error_table_is_listed_and_the_list_is_sorted() {
    for code in [
        "PKG_AGENT_UNKNOWN",
        "PKG_NOT_AN_AGENT",
        "PKG_AGENT_NOT_OFFERED",
        "PKG_DEPRECATED_AS_AGENT",
        "PKG_ALREADY_ADDED",
        "PKG_UNSAFE",
        "PKG_NEEDS_IT_APPROVAL",
        "PKG_PACK_MISSING",
        "PKG_CHECK_UNAVAILABLE",
        "PKG_INSTALL_FAILED",
        "PKG_CHOICE_NOT_ALLOWED",
        "PKG_NO_UPDATE",
        "PKG_INSTANCE_UNKNOWN",
        "PKG_INSTANCE_NOT_READY",
        "PKG_FOLDER_UNUSABLE",
        "PKG_TRIAL_NEEDS_TRUST",
        "PKG_FOLDER_MISMATCH",
        "PKG_AGENT_NAME_AMBIGUOUS",
        "PKG_TRIAL_NOT_NAMEABLE",
        "PKG_TRIAL_NO_SCOPE",
        "PKG_NEED_INVALID",
        "PKG_LIBRARY_UNAVAILABLE",
        "PKG_STORE_FAILED",
        "PKG_BUNDLED_AGENT_FIXED",
        "PKG_BUNDLED_AGENT_MISSING",
        "PKG_BUNDLED_AGENT_REFUSED",
    ] {
        assert!(ALL_CODES.contains(&code), "{code}");
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, ALL_CODES);
}

#[test]
fn the_migrations_are_module_package_catalogue_with_two_sqlite_steps_and_no_postgres_step() {
    let migrations = crate::migrations();
    assert_eq!(migrations.module, "package_catalogue");
    // Step 2 is PKG-DOC-2's `starter_agent_offers`.
    assert_eq!(migrations.sqlite.len(), 2);
    assert_eq!(migrations.sqlite[0].0, 1);
    assert_eq!(migrations.sqlite[1].0, 2);
    assert!(migrations.postgres.is_empty());
}
