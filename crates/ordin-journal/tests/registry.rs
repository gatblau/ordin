//! X4: every code this crate exports in `ALL_CODES` is registered, owned by this crate under a
//! prefix it owns, and has the retry flag the error type gives it.

use ordin_journal::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

const PACKAGE: &str = "ordin-journal";

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    assert_eq!(ALL_CODES.len(), 48);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
        assert_eq!(
            entry.retryable,
            *code == "JRNL_DATASTORE_UNAVAILABLE",
            "{code}"
        );
    }
}

#[test]
fn the_cursor_code_this_crate_raises_is_registered_to_shared_types() {
    let entry = lookup("SRV_BAD_CURSOR").expect("registered");
    assert_eq!(entry.owner_crate, "ordin-types");
    assert!(!ALL_CODES.contains(&"SRV_BAD_CURSOR"));
}
