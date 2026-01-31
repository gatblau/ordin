//! X4: every code this crate exports in `ALL_CODES` is registered, owned by this crate under a
//! prefix it owns, and every code the registry gives this crate is exported.

use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_workspaces::{WorkspaceError, ALL_CODES};

const PACKAGE: &str = "ordin-workspaces";

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    assert_eq!(ALL_CODES.len(), 56);
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

/// The Error Table's status column against the registry's `retryable` flag, for every error value
/// this crate can build.
#[test]
fn the_error_type_reports_the_registrys_codes_and_retry_flags() {
    use ordin_types::domain::places::{WorkingCopyState, WorkspaceState};
    let errors = [
        WorkspaceError::NotFound,
        WorkspaceError::NotActive {
            state: WorkspaceState::InBin,
        },
        WorkspaceError::StateInvalid,
        WorkspaceError::OwnFolderFailed,
        WorkspaceError::LinkNotFound,
        WorkspaceError::LinkStateInvalid,
        WorkspaceError::NotARepository,
        WorkspaceError::PlaceRefused { cause: "protected" },
        WorkspaceError::SourceSignInNeeded,
        WorkspaceError::SourceNotConfigured,
        WorkspaceError::SourceAccessDenied,
        WorkspaceError::SourceNotFound,
        WorkspaceError::SourceUnavailable,
        WorkspaceError::ConfluenceUnavailable,
        WorkspaceError::ItemTooLarge,
        WorkspaceError::WorkingCopyTransitionInvalid {
            from: WorkingCopyState::Read,
            to: WorkingCopyState::Pushed,
        },
        WorkspaceError::GrantPublishFailed,
        WorkspaceError::FilesFailed,
        WorkspaceError::DatastoreUnavailable,
        WorkspaceError::LocalFolderOnServer,
        WorkspaceError::WorkingCopyItemNotFound,
        WorkspaceError::FileNameInvalid,
        WorkspaceError::FileTooLarge,
        WorkspaceError::GitRemoteInvalid,
        WorkspaceError::JiraUnavailable,
        WorkspaceError::JiraAddressInvalid,
        WorkspaceError::JiraSiteMismatch,
        WorkspaceError::JiraReadOnly,
        WorkspaceError::OriginInvalid,
        WorkspaceError::NotTeam,
        WorkspaceError::ResultPathInvalid,
        WorkspaceError::ResultPathTaken,
        WorkspaceError::GrantNotFound,
    ];
    let mut seen = Vec::new();
    for error in &errors {
        let entry =
            lookup(error.code()).unwrap_or_else(|| panic!("{} is not registered", error.code()));
        assert_eq!(error.to_string(), error.code());
        assert_eq!(entry.retryable, error.retryable(), "{}", error.code());
        assert!(ALL_CODES.contains(&error.code()));
        seen.push(error.code());
    }
    // Every code of the registry that is not a 2C validation row has a variant of its own.
    for code in ALL_CODES {
        let validation = [
            "CFL_",
            "GRANT_",
            "MAIL_",
            "SRC_",
            "WCI_",
            "WS_BIN_MISMATCH",
            "WS_NAME_",
        ]
        .iter()
        .any(|p| code.starts_with(p));
        if !validation {
            assert!(seen.contains(code), "{code} has no WorkspaceError variant");
        }
    }
    for code in [
        "WS_NAME_EMPTY",
        "SRC_DUPLICATE",
        "WCI_DUPLICATE",
        "GRANT_DUPLICATE",
        "CFL_NOT_APPROVED",
        "MAIL_ADDRESS_INVALID",
    ] {
        let error = WorkspaceError::Invalid(lookup(code).expect("registered").code);
        assert_eq!(error.code(), code);
        assert_eq!(lookup(code).unwrap().retryable, error.retryable(), "{code}");
    }
}
