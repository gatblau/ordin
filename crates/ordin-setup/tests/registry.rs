//! X4: every code this crate exports in `ALL_CODES` is registered, owned by this crate under a
//! prefix it owns, with the sentence id the registry derives from the code.

use ordin_setup::{SetupError, ALL_CODES};
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::CodedError;

const PACKAGE: &str = "ordin-setup";

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    assert_eq!(ALL_CODES.len(), 38);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        assert_eq!(
            entry.sentence_id,
            code.to_lowercase().replace('_', "-"),
            "{code}"
        );
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

/// The error type reports its code alone as `Display`, with the retry flag of its variant.
#[test]
fn the_error_type_reports_its_code_and_flag() {
    for code in ALL_CODES {
        let entry = lookup(code).expect("registered");
        let error = if entry.retryable {
            SetupError::Retryable { code }
        } else {
            SetupError::Permanent { code }
        };
        assert_eq!(error.code(), *code);
        assert_eq!(error.to_string(), *code);
        assert_eq!(error.retryable(), entry.retryable, "{code}");
    }
}
