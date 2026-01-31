//! X4: every code in `ALL_CODES` is registered, owned by this crate, and its prefix is this crate's
//! (ErrorHandling.md *Internal Logic* 11).

use std::collections::BTreeSet;

use ordin_installers::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

const PACKAGE: &str = "ordin-installers";

#[test]
fn every_code_is_registered_and_owned() {
    assert_eq!(ALL_CODES.len(), 11);
    let distinct: BTreeSet<&&str> = ALL_CODES.iter().collect();
    assert_eq!(distinct.len(), ALL_CODES.len(), "a code is listed twice");
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert_eq!(prefix, "INST", "{code}");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
    }
}
