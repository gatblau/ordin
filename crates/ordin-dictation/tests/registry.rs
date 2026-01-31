//! Every `TRX_` code of the Transcription Service is registered to this crate under a prefix it owns,
//! with the retry flag its error gives it (ErrorHandling.md *Internal Logic* 11).

use ordin_dictation::transcription::ALL_CODES;
use ordin_types::error_registry::{lookup, owns_prefix};

#[test]
fn every_transcription_code_is_registered_under_a_prefix_this_crate_owns() {
    assert_eq!(ALL_CODES.len(), 13);
    let retryable = [
        "TRX_BUSY",
        "TRX_ENGINE_FAILED",
        "TRX_MODEL_CHECKSUM",
        "TRX_MODEL_DOWNLOAD_FAILED",
    ];
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, "ordin-dictation", "{code}");
        assert!(code.starts_with("TRX_"));
        assert!(owns_prefix("ordin-dictation", "TRX"));
        assert_eq!(entry.retryable, retryable.contains(code), "{code}");
    }
}
