//! X4: every code in `ALL_CODES` is registered, owned by this crate under the `CRED` prefix, and the
//! registry's retry flag agrees with the Error Table's status.

use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::CodedError as _;

use crate::error::CredError;
use crate::ALL_CODES;

const PACKAGE: &str = "ordin-identity";

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    assert_eq!(ALL_CODES.len(), 39, "one code per Error Table row");
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
        assert_eq!(
            CredError::of(code).retryable(),
            entry.retryable,
            "{code}: retry flag differs from the registry"
        );
    }
}

#[test]
fn display_writes_the_code_only() {
    for code in ALL_CODES {
        assert_eq!(CredError::of(code).to_string(), *code);
    }
}
