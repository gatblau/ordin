//! X4: every code this crate exports in `ALL_CODES` is registered, owned by this crate under a
//! prefix it owns, and has the retry flag the error type gives it.

use ordin_agent_folders::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

const PACKAGE: &str = "ordin-agent-folders";

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    assert_eq!(ALL_CODES.len(), 8);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
        // The registry flag is the one a caller can rely on without the error value: the codes
        // whose error carries its own flag (`AFM_DATASTORE_FAILED`) register `false`.
        let expected = matches!(*code, "AFM_FOLDER_BUSY" | "AFM_IO_FAILED");
        assert_eq!(entry.retryable, expected, "{code}");
    }
}

#[test]
fn the_registry_holds_no_afm_code_this_crate_does_not_export() {
    // `AFM_PATH_INVALID` went with `check_access` (SA-CLOSE-01).
    assert!(lookup("AFM_PATH_INVALID").is_none());
}
