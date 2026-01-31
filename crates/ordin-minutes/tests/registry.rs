//! Every `MIN_` code of the Minutes Service is registered to this crate under a prefix it owns,
//! with the retry flag its error gives it (ErrorHandling.md *Internal Logic* 11).

use ordin_minutes::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

#[test]
fn every_min_code_is_registered_under_a_prefix_this_crate_owns() {
    // 13 with MIN_JOB_NOT_READY (MinutesService.md MIN-TR-3).
    assert_eq!(ALL_CODES.len(), 13);
    let retryable = ["MIN_STORE_FAILED", "MIN_WRITE_FAILED"];
    assert!(owns_prefix("ordin-minutes", "MIN"));
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, "ordin-minutes", "{code}");
        assert!(code.starts_with("MIN_"));
        assert_eq!(entry.retryable, retryable.contains(code), "{code}");
    }
}
