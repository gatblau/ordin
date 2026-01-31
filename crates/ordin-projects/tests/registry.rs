//! X4: every code this crate exports in `ALL_CODES` is registered, owned by this crate under a
//! prefix it owns.

use ordin_projects::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

const PACKAGE: &str = "ordin-projects";

/// The Error Table's status column, per code. `PROJ_IDE_NOT_FOUND` is `Err(permanent)` in the spec
/// and retryable in the registry: the crate follows the spec, and the registry row is reported.
const RETRYABLE: &[&str] = &[
    "GH_RATE_LIMITED",
    "GH_UNREACHABLE",
    "PROJ_DATASTORE_UNAVAILABLE",
    "PROJ_FILES_FAILED",
    "PROJ_IDE_START_FAILED",
    "PROJ_LSP_UNAVAILABLE",
    "PROJ_PUSH_REFUSED",
    "PROJ_RATE_LIMITED",
    "PROJ_REMOTE_UNREACHABLE",
    "PROJ_SERVER_INSTALL_FAILED",
    "PROJ_TERMINAL_FAILED",
    "PROJ_WORKTREE_FAILED",
];

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    assert_eq!(ALL_CODES.len(), 52);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
        if *code != "PROJ_IDE_NOT_FOUND" {
            assert_eq!(entry.retryable, RETRYABLE.contains(code), "{code}");
        }
    }
}

#[test]
fn the_codes_are_sorted_and_unique() {
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, ALL_CODES);
}
