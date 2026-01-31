//! X4: every code in `ALL_CODES` is registered, and the prefixes are owned by this crate.

use ordin_catalogue::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

#[test]
fn every_code_is_found_by_the_registry_and_its_prefix_is_owned() {
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        if *code == "SRV_BAD_CURSOR" {
            assert_eq!(entry.owner_crate, "ordin-types");
            continue;
        }
        assert_eq!(entry.owner_crate, "ordin-catalogue", "{code}");
        let prefix = code.split('_').next().unwrap();
        assert!(
            owns_prefix("ordin-catalogue", prefix),
            "{prefix} is not owned by ordin-catalogue"
        );
    }
}

#[test]
fn every_code_of_the_registry_this_crate_owns_is_listed() {
    // The registry's own table is compiled into `ordin-types`; the codes it gives this crate are the
    // ones the Error Table and the 2C rules name.
    for code in [
        "AREPO_ADD_DISABLED",
        "AREPO_DUPLICATE",
        "CAT_BRANCH_EXISTS",
        "CAT_PROFILE_KEY_RESERVED",
        "LIFE_PLACE_INVALID",
        "LIFE_SLACK_CHANNEL_INVALID",
        "PART_DUPLICATE",
        "VOCAB_CODE_INVALID",
        "WDL_APPEND_ONLY",
    ] {
        assert!(ALL_CODES.contains(&code), "{code}");
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted, ALL_CODES,
        "ALL_CODES is sorted and free of duplicates"
    );
}

#[test]
fn the_migrations_are_module_catalogue_with_two_sqlite_steps_and_no_postgres_step() {
    let migrations = ordin_catalogue::migrations();
    assert_eq!(migrations.module, "catalogue");
    assert_eq!(
        migrations.sqlite.iter().map(|s| s.0).collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(migrations.postgres.is_empty());
}
