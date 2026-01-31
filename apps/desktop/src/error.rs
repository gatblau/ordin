//! The program's start error and its error codes (OrdinDesktop.md *Rust items of the program*,
//! *Error Table*). Prefix `DSK_` (*Approach*: `DESK_` belongs to the Remote API).

/// Every code this program raises or logs, in the order of the Error Table and its amendments
/// (ErrorHandling.md *Internal Logic* 11; Playbook X4).
pub const ALL_CODES: &[&str] = &[
    DATA_ROOT_UNWRITABLE,
    START_FAILED,
    AGENT_PROFILE_ONLY,
    PREFERENCES_WRITE_FAILED,
    PREFERENCE_VALUE_INVALID,
    PREFERENCES_UNREADABLE,
    DIALOG_CANCELLED,
    SAVE_COPY_FAILED,
    PICKER_UNAVAILABLE,
    NOTIFY_FAILED,
    LINK_REGISTER_FAILED,
    LINK_REFUSED,
    RELAY_URL_REFUSED,
    RELAY_TIMEOUT,
    DRAFT_LIST_FAILED,
    SIDECAR_MISSING,
    SHUTDOWN_STEP_TIMEOUT,
    UPDATE_CHECK_FAILED,
    UPDATE_INSTALL_FAILED,
    WRITES_NOT_ATTRIBUTED,
    MODELS_NOT_IN_BUILD,
    MODELS_OFFLINE,
    MODELS_BLOCKED,
    MODELS_MISMATCH,
    MODELS_DISK_FULL,
    MODELS_INTERRUPTED,
    MODELS_WRITE_FAILED,
];

pub const DATA_ROOT_UNWRITABLE: &str = "DSK_DATA_ROOT_UNWRITABLE";
pub const START_FAILED: &str = "DSK_START_FAILED";
pub const AGENT_PROFILE_ONLY: &str = "DSK_AGENT_PROFILE_ONLY";
pub const PREFERENCES_WRITE_FAILED: &str = "DSK_PREFERENCES_WRITE_FAILED";
pub const PREFERENCE_VALUE_INVALID: &str = "DSK_PREFERENCE_VALUE_INVALID";
pub const PREFERENCES_UNREADABLE: &str = "DSK_PREFERENCES_UNREADABLE";
pub const DIALOG_CANCELLED: &str = "DSK_DIALOG_CANCELLED";
pub const SAVE_COPY_FAILED: &str = "DSK_SAVE_COPY_FAILED";
pub const PICKER_UNAVAILABLE: &str = "DSK_PICKER_UNAVAILABLE";
pub const NOTIFY_FAILED: &str = "DSK_NOTIFY_FAILED";
pub const LINK_REGISTER_FAILED: &str = "DSK_LINK_REGISTER_FAILED";
pub const LINK_REFUSED: &str = "DSK_LINK_REFUSED";
pub const RELAY_URL_REFUSED: &str = "DSK_RELAY_URL_REFUSED";
pub const RELAY_TIMEOUT: &str = "DSK_RELAY_TIMEOUT";
pub const DRAFT_LIST_FAILED: &str = "DSK_DRAFT_LIST_FAILED";
pub const SIDECAR_MISSING: &str = "DSK_SIDECAR_MISSING";
pub const SHUTDOWN_STEP_TIMEOUT: &str = "DSK_SHUTDOWN_STEP_TIMEOUT";
pub const UPDATE_CHECK_FAILED: &str = "DSK_UPDATE_CHECK_FAILED";
pub const UPDATE_INSTALL_FAILED: &str = "DSK_UPDATE_INSTALL_FAILED";
/// The Hook Relay attributes no writes (amendment January 2026, DSK-WIRE-2): the `code` of the
/// `StartError::ModuleFailed` for module `ordin-hook-relay`.
// spec: ordin-desktop/components/OrdinDesktop#DSK-WIRE-2
pub const WRITES_NOT_ATTRIBUTED: &str = "DSK_WRITES_NOT_ATTRIBUTED";

// The memory models download (amendment 2026-01-28, *Error Table (rows added)*). Only
// `DSK_MODELS_NOT_IN_BUILD` is a command error; the other six arrive in `MemoryModelsView.failure`.
/// `app.download_memory_models` in a build with no models input (DSK-MD-2).
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-6
pub const MODELS_NOT_IN_BUILD: &str = "DSK_MODELS_NOT_IN_BUILD";
/// No connection to the models' host after 6 attempts, and no byte received (DSK-MD-7).
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-7
pub const MODELS_OFFLINE: &str = "DSK_MODELS_OFFLINE";
/// The host refuses the address, a redirect is refused, or the certificate is refused (DSK-MD-6).
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-6
pub const MODELS_BLOCKED: &str = "DSK_MODELS_BLOCKED";
/// A fingerprint, an entry or a size differs, or the re-check fails (DSK-MD-8 to DSK-MD-10).
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-9
pub const MODELS_MISMATCH: &str = "DSK_MODELS_MISMATCH";
/// The disk is full while writing the zip or unpacking it (DSK-MD-8, DSK-MD-9).
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-8
pub const MODELS_DISK_FULL: &str = "DSK_MODELS_DISK_FULL";
/// 6 attempts used up after bytes had arrived (DSK-MD-7).
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-7
pub const MODELS_INTERRUPTED: &str = "DSK_MODELS_INTERRUPTED";
/// Writing, unpacking or renaming fails for a reason other than a full disk (DSK-MD-8 to DSK-MD-10).
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-10
pub const MODELS_WRITE_FAILED: &str = "DSK_MODELS_WRITE_FAILED";

/// Why start stopped (*Internal Logic* step 9). Both variants are permanent: start stops, a message
/// box is shown and the program exits with status 1.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("DSK_DATA_ROOT_UNWRITABLE")]
    DataRootUnwritable {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    /// `module` is the crate whose constructor, binding or start failed; `code` is that module's own
    /// error code, which the `start_failed` log line names.
    #[error("DSK_START_FAILED")]
    ModuleFailed { module: &'static str, code: String },
}

impl StartError {
    /// The code of the `start_failed` log line.
    pub fn code(&self) -> &'static str {
        match self {
            StartError::DataRootUnwritable { .. } => DATA_ROOT_UNWRITABLE,
            StartError::ModuleFailed { .. } => START_FAILED,
        }
    }

    /// The module the `start_failed` log line names; `None` when start stopped before any module.
    pub fn module(&self) -> Option<&'static str> {
        match self {
            StartError::DataRootUnwritable { .. } => None,
            StartError::ModuleFailed { module, .. } => Some(module),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::error_registry::{lookup, owns_prefix};

    #[test]
    fn every_code_is_registered_and_the_prefix_is_owned() {
        for code in ALL_CODES {
            let entry = lookup(code).unwrap_or_else(|| panic!("{code} is registered"));
            assert_eq!(entry.owner_crate, "ordin-desktop", "{code}");
            let prefix = code.split('_').next().expect("prefix");
            assert!(owns_prefix("ordin-desktop", prefix), "{code}");
        }
    }

    #[test]
    fn start_errors_carry_their_codes() {
        let unwritable = StartError::DataRootUnwritable {
            path: "/nowhere".into(),
            source: std::io::Error::other("refused"),
        };
        assert_eq!(unwritable.code(), DATA_ROOT_UNWRITABLE);
        assert_eq!(unwritable.to_string(), DATA_ROOT_UNWRITABLE);
        assert_eq!(unwritable.module(), None);
        let failed = StartError::ModuleFailed {
            module: "ordin-sharepoint",
            code: "SP_DATASTORE_UNAVAILABLE".into(),
        };
        assert_eq!(failed.code(), START_FAILED);
        assert_eq!(failed.module(), Some("ordin-sharepoint"));
        // Both are permanent: the registry marks neither retryable.
        for code in [DATA_ROOT_UNWRITABLE, START_FAILED] {
            assert!(!lookup(code).expect("registered").retryable, "{code}");
        }
    }
}
