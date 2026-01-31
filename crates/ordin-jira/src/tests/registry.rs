//! X4: every code in `ALL_CODES` is registered, owned by this crate under the `JIRA` prefix, and the
//! registry's retry flag agrees with the Error Table's status.

use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::{CodedError, RepairAction};

use crate::{JiraError, ALL_CODES};

const PACKAGE: &str = "ordin-jira";

fn every_variant() -> Vec<JiraError> {
    vec![
        JiraError::AddressInvalid,
        JiraError::NotConnected,
        JiraError::SignInNeeded,
        JiraError::TokenRefused,
        JiraError::SiteMismatch,
        JiraError::AccessDenied,
        JiraError::NotFound,
        JiraError::QueryInvalid,
        JiraError::IssueTypeNone,
        JiraError::TooLarge,
        JiraError::Throttled,
        JiraError::Unavailable,
        JiraError::ResponseInvalid,
        JiraError::OutcomeUnknown,
        JiraError::ActionKeyUsed,
    ]
}

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    assert_eq!(ALL_CODES.len(), 15, "one code per Error Table row");
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
fn every_variant_has_a_listed_code_and_the_registry_retry_flag() {
    let errors = every_variant();
    assert_eq!(errors.len(), ALL_CODES.len());
    for error in errors {
        let code = error.code();
        assert!(ALL_CODES.contains(&code), "{code} is not in ALL_CODES");
        let entry = lookup(code).expect("registered");
        assert_eq!(error.retryable(), entry.retryable, "{code}");
        assert_eq!(CodedError::retryable(&error), entry.retryable, "{code}");
    }
}

#[test]
fn display_writes_the_code_only() {
    for error in every_variant() {
        assert_eq!(error.to_string(), error.code());
    }
}

#[test]
fn only_throttled_and_unavailable_are_retryable() {
    let retryable: Vec<&str> = every_variant()
        .iter()
        .filter(|e| e.retryable())
        .map(JiraError::code)
        .collect();
    assert_eq!(retryable, ["JIRA_THROTTLED", "JIRA_UNAVAILABLE"]);
}

#[test]
fn repair_actions_follow_the_response_body_column() {
    let repair = |e: JiraError| CodedError::repair(&e);
    assert_eq!(
        repair(JiraError::NotConnected),
        Some(RepairAction::SignInAgain)
    );
    assert_eq!(
        repair(JiraError::SignInNeeded),
        Some(RepairAction::SignInAgain)
    );
    assert_eq!(repair(JiraError::Throttled), Some(RepairAction::TryAgain));
    assert_eq!(repair(JiraError::Unavailable), Some(RepairAction::TryAgain));
    assert_eq!(repair(JiraError::ResponseInvalid), None);
    assert_eq!(repair(JiraError::AddressInvalid), None);
    assert_eq!(repair(JiraError::OutcomeUnknown), None);
    assert_eq!(repair(JiraError::ActionKeyUsed), None);
}
