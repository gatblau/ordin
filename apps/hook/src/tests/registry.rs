//! X4: every code in `ALL_CODES` is registered and under a prefix this program owns.

use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::CodedError as _;

use crate::error::HookFailure;
use crate::ALL_CODES;

const PACKAGE: &str = "ordin-hook";

#[test]
fn every_code_of_this_program_is_registered_under_a_prefix_it_owns() {
    assert_eq!(
        ALL_CODES.len(),
        13,
        "one code per Error Table row raised here"
    );
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        // HOOK_APPROVAL_PENDING is raised at both ends of the protocol (OrdinHook.md Error Table).
        if *code == "HOOK_APPROVAL_PENDING" {
            assert_eq!(entry.owner_crate, "ordin-hook-relay");
        } else {
            assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        }
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
        assert!(!entry.retryable, "{code}");
    }
}

#[test]
fn codes_are_sorted_and_unique() {
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, ALL_CODES);
}

#[test]
fn failures_write_the_code_only() {
    let failures = [
        HookFailure::env_invalid(),
        HookFailure::payload_invalid(),
        HookFailure::request_too_large(),
        HookFailure::core_unreachable(),
        HookFailure::core_timeout(),
        HookFailure::reply_malformed(),
        HookFailure::approval_pending(),
        HookFailure::output_failed(),
        HookFailure::bridge_env_invalid(),
        HookFailure::key_unreadable(),
    ];
    for failure in failures {
        assert_eq!(failure.to_string(), failure.code);
        assert_eq!(failure.code(), failure.code);
        assert!(!failure.retryable());
        assert!(failure.repair().is_none());
        assert!(ALL_CODES.contains(&failure.code));
    }
}
