//! Every `OBS_` code of the OBS Recorder Adapter is registered to this crate under a prefix it owns,
//! with the retry flag its error gives it (ErrorHandling.md *Internal Logic* 11).

use ordin_obs::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

#[test]
fn every_obs_code_is_registered_under_a_prefix_this_crate_owns() {
    assert_eq!(ALL_CODES.len(), 12);
    let retryable = [
        "OBS_NOT_REACHABLE",
        "OBS_REQUEST_FAILED",
        "OBS_SCENE_FAILED",
        "OBS_TIMEOUT",
    ];
    assert!(owns_prefix("ordin-obs", "OBS"));
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, "ordin-obs", "{code}");
        assert!(code.starts_with("OBS_"));
        assert_eq!(entry.retryable, retryable.contains(code), "{code}");
    }
}
