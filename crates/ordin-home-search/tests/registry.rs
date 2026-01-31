//! X4: every code this crate exports in `ALL_CODES` is registered, owned by this crate under a
//! prefix it owns, and every code the registry gives this crate is exported.

use ordin_home_search::{HomeError, ALL_CODES};
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::CodedError;

const PACKAGE: &str = "ordin-home-search";

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    assert_eq!(ALL_CODES.len(), 7);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
    }
}

#[test]
fn the_codes_are_sorted_and_unique() {
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, ALL_CODES);
}

/// Every value of the error type reports an exported code; the retry flag follows the Error
/// Table, which the registry agrees with except for `HOME_SOURCE_UNAVAILABLE`, registered
/// "yes / no (per row)": retryable where it is returned (`needs_you`), logged elsewhere.
#[test]
fn the_error_type_reports_the_registrys_codes_and_retry_flags() {
    let errors = [
        HomeError::QueryInvalid,
        HomeError::WorkspaceNotFound,
        HomeError::RunNotFound,
        HomeError::SourceUnavailable,
        HomeError::DatastoreUnavailable,
        HomeError::Invalid("HOME_MARK_DUPLICATE"),
        HomeError::Invalid("HOME_PATH_INVALID"),
    ];
    let mut seen: Vec<&str> = errors.iter().map(|e| e.code()).collect();
    seen.sort_unstable();
    assert_eq!(seen, ALL_CODES);
    for error in errors {
        let entry = lookup(error.code()).expect("registered");
        assert_eq!(error.to_string(), error.code());
        if error.code() == "HOME_SOURCE_UNAVAILABLE" {
            assert!(error.retryable());
        } else {
            assert_eq!(error.retryable(), entry.retryable, "{}", error.code());
        }
    }
}
