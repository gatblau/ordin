//! X4: every code in `ALL_CODES` is registered, owned by this crate, and not retryable
//! (ErrorHandling.md *Internal Logic* 11).

use ordin_format_plugins::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

const PACKAGE: &str = "ordin-format-plugins";

#[test]
fn every_code_is_registered_and_owned() {
    assert_eq!(ALL_CODES.len(), 10);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        assert!(!entry.retryable, "{code}");
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
    }
}
