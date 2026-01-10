//! X4: every code this crate exports in `ALL_CODES` is registered and its prefix is this crate's;
//! and the registry's own lookups (ErrorHandling.md *Public Interface*, *Functions*).

use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::ALL_CODES;

const PACKAGE: &str = "ordin-types";

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.code, *code);
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
        assert!(
            !entry.retryable,
            "{code}: no error of this crate is retryable"
        );
    }
}

#[test]
fn all_codes_lists_every_code_the_registry_gives_shared_types() {
    // The registry rows whose owner is Shared Types (ErrorHandling.md): 13 codes.
    assert_eq!(ALL_CODES.len(), 13);
    for code in ALL_CODES {
        assert_eq!(lookup(code).map(|e| e.owner_crate), Some(PACKAGE), "{code}");
    }
}

#[test]
fn lookup_reads_one_row_with_its_columns() {
    let entry = lookup("APPR_CARD_ALREADY_DECIDED").expect("registered");
    assert_eq!(entry.owner_crate, "ordin-approval");
    assert_eq!(entry.http_status, Some(200));
    assert!(!entry.retryable);
    assert_eq!(entry.sentence_id, "appr-card-already-decided");

    let deadline = lookup("SVC_DEADLINE").expect("registered");
    assert!(deadline.retryable);
    assert_eq!(deadline.http_status, Some(503));

    // A row with no HTTP surface.
    assert_eq!(
        lookup("HOOK_APPROVAL_PENDING").map(|e| e.http_status),
        Some(None)
    );
}

#[test]
fn lookup_refuses_an_unregistered_code() {
    assert!(lookup("XYZ_UNKNOWN").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("appr_card_already_decided").is_none());
}

#[test]
fn owns_prefix_follows_the_prefix_table() {
    assert!(owns_prefix("ordin-types", "SRV"));
    assert!(owns_prefix("ordin-caller-api", "SRV"));
    assert!(owns_prefix("ordin-remote-api", "SRV"));
    assert!(owns_prefix("ordin-mail", "APPR"));
    assert!(owns_prefix("ordin-arch-test", "ARCH"));
    assert!(owns_prefix("ordin-server", "SERVER"));
    assert!(!owns_prefix("ordin-types", "APPR"));
    assert!(!owns_prefix("ordin-types", "SRV_"));
    assert!(!owns_prefix("ordin-types", "NOPE"));
}

#[test]
fn the_registry_matches_the_arch_test_codes() {
    // Step 1's crate exports twelve `ARCH_` codes; each must be registered (Internal Logic 11 (a)).
    for code in [
        "ARCH_CYCLE",
        "ARCH_DEV_EDGE_NOT_ALLOWED",
        "ARCH_EDGE_NOT_ALLOWED",
        "ARCH_EMBEDDED_EDGE",
        "ARCH_METADATA_FAILED",
        "ARCH_METADATA_INVALID",
        "ARCH_PERSON_ONLY_CALLED",
        "ARCH_PERSON_ONLY_NAMED",
        "ARCH_SOURCE_UNREADABLE",
        "ARCH_TABLE_INVALID",
        "ARCH_UNKNOWN_CRATE",
        "ARCH_UNKNOWN_PATH_PACKAGE",
    ] {
        assert_eq!(
            lookup(code).map(|e| e.owner_crate),
            Some("ordin-arch-test"),
            "{code}"
        );
    }
}
