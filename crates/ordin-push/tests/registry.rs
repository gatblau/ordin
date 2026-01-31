//! X4: every code this crate owns is registered under it, and the repair actions and retry flags of
//! `PushError::new` match the Error Table and the registry.
use ordin_push::{PushError, ALL_CODES};
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::CodedError;

#[test]
fn every_owned_code_is_registered_and_the_prefix_is_owned() {
    assert_eq!(ALL_CODES.len(), 38);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-push", "{code}");
        assert!(owns_prefix("ordin-push", code.split('_').next().unwrap()));
        // The registry and `PushError::new` agree on whether the same call may succeed later.
        let error = PushError::new(code, "");
        assert_eq!(CodedError::retryable(&error), entry.retryable, "{code}");
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted, ALL_CODES,
        "ALL_CODES is sorted and has no duplicates"
    );
}

#[test]
fn the_repair_action_of_each_row_is_the_error_tables() {
    use ordin_types::RepairAction::{SignInAgain, TryAgain};
    let cases = [
        ("PUSH_APPROVAL_FAILED", Some(TryAgain)),
        ("PUSH_ORIGIN_UNREACHABLE", Some(TryAgain)),
        ("PUSH_STORE_FAILED", Some(TryAgain)),
        ("PUSH_ORIGINAL_UNREADABLE", Some(TryAgain)),
        ("PUSH_COPY_MAKER_FAILED", Some(TryAgain)),
        ("PUSH_SIGN_IN_NEEDED", Some(SignInAgain)),
        ("PUSH_DESTINATION_REFUSED", None),
        ("PUSH_GIT_IDENTITY_MISSING", None),
        ("PUSH_ALL_ORIGINS_CHANGED", None),
        ("PUSH_OUTCOME_UNKNOWN", None),
        ("PUSH_FORBIDDEN", None),
        ("PUSH_APPROVED_VERSION_CHANGED", None),
    ];
    for (code, action) in cases {
        assert_eq!(
            CodedError::repair(&PushError::new(code, "")),
            action,
            "{code}"
        );
    }
}

// PUSH_COPY_MAKER_FAILED is tested end to end in `conversion.rs`, with a fake conversion app (FU-105).
#[test]
fn the_copy_maker_failure_is_a_try_again_code_that_is_not_retryable() {
    let error = PushError::new("PUSH_COPY_MAKER_FAILED", "CRAFT_TOOL_FAILED");
    assert!(!error.retryable);
    assert_eq!(error.to_string(), "PUSH_COPY_MAKER_FAILED");
}
