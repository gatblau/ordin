//! X4: every code in `ALL_CODES` is registered, the `KQ` prefix is owned by this crate, and the
//! registry's retry flag agrees with the Error Table.

use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::{CodedError, RepairAction};

use crate::{KqError, ALL_CODES};

const PACKAGE: &str = "ordin-knowledge-query-client";

fn every_variant() -> Vec<KqError> {
    vec![
        KqError::UrlInvalid,
        KqError::TooManyConnections,
        KqError::AlreadyConnected,
        KqError::NotAKnowledgeAgent,
        KqError::OtherCompany,
        KqError::SignInNeeded,
        KqError::NotInAudience,
        KqError::Unreachable,
        KqError::ConnectionNotFound,
        KqError::NotSupported,
        KqError::ConfigInvalid,
        KqError::BadQuery,
        KqError::NoConnections,
        KqError::UnknownProxy,
        KqError::StoreUnavailable,
    ]
}

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    assert_eq!(ALL_CODES.len(), 15, "one code per Error Table row");
    for code in ALL_CODES {
        assert!(lookup(code).is_some(), "{code} is not registered");
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
        assert_eq!(CodedError::code(&error), code);
    }
}

#[test]
fn display_writes_the_code_only() {
    for error in every_variant() {
        assert_eq!(error.to_string(), error.code());
    }
}

#[test]
fn only_unreachable_and_store_unavailable_are_retryable() {
    let retryable: Vec<&str> = every_variant()
        .iter()
        .filter(|e| e.retryable())
        .map(KqError::code)
        .collect();
    assert_eq!(retryable, ["KQ_UNREACHABLE", "KQ_STORE_UNAVAILABLE"]);
}

#[test]
fn repair_actions_name_what_the_person_can_do() {
    let repair = |e: KqError| CodedError::repair(&e);
    assert_eq!(
        repair(KqError::SignInNeeded),
        Some(RepairAction::SignInAgain)
    );
    assert_eq!(repair(KqError::Unreachable), Some(RepairAction::TryAgain));
    assert_eq!(
        repair(KqError::StoreUnavailable),
        Some(RepairAction::TryAgain)
    );
    assert_eq!(repair(KqError::UrlInvalid), None);
}

#[test]
fn a_store_error_is_store_unavailable() {
    assert_eq!(
        KqError::from(sqlx::Error::RowNotFound),
        KqError::StoreUnavailable
    );
}
