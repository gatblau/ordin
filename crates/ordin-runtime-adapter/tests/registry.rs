//! X4: every code this crate exports in `ALL_CODES` is registered under a prefix it owns, and the
//! registry's retryability and status columns agree with the Error Table.

use ordin_runtime_adapter::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

const PACKAGE: &str = "ordin-runtime-adapter";

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.code, *code);
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        assert!(code.starts_with("RA_"), "{code}");
        assert!(owns_prefix(PACKAGE, "RA"), "{PACKAGE} does not own RA_");
    }
}

#[test]
fn all_codes_holds_the_23_codes_of_the_prefix_once_each_in_order() {
    // ErrorHandling.md prefix table: `RA_`, 23 codes.
    assert_eq!(ALL_CODES.len(), 23);
    assert!(
        ALL_CODES.windows(2).all(|w| w[0] < w[1]),
        "sorted, no repeats"
    );
}

#[test]
fn registry_rows_carry_the_retryability_and_status_of_the_error_table() {
    let retryable = ["RA_DEADLINE", "RA_THREAD_BUSY", "RA_UNREACHABLE"];
    let internal = ["RA_EVENT_INVALID", "RA_EVENT_ORDER", "RA_PLAN_INVALID"];
    for code in ALL_CODES {
        let entry = lookup(code).expect("registered");
        assert_eq!(entry.retryable, retryable.contains(code), "{code}");
        let status = if internal.contains(code) {
            None
        } else {
            Some(200)
        };
        assert_eq!(entry.http_status, status, "{code}");
    }
}
