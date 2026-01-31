//! X4: every code in `ALL_CODES` is registered, and its prefix is owned by this crate
//! (ErrorHandling.md *Internal Logic* 11).

use ordin_resolver::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

#[test]
fn every_code_is_registered_and_owned() {
    assert_eq!(ALL_CODES.len(), 32);
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, ALL_CODES, "ALL_CODES stays in registry order");
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, "ordin-resolver", "{code}");
        let prefix = code.split('_').next().expect("a prefix");
        assert!(owns_prefix("ordin-resolver", prefix), "{prefix}");
    }
}
