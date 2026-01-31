//! The crate's one error type and the codes it owns (PackageInspector.md *Error Table*).

use ordin_types::CodedError;

/// Every code the registry gives this crate (its `PKI_` rows), sorted.
pub const ALL_CODES: &[&str] = &[
    "PKI_BUNDLE_UNREADABLE",
    "PKI_DECLARATION_INVALID",
    "PKI_DECLARATION_REFUSED",
    "PKI_DECLARATION_TOO_LARGE",
    "PKI_FOLDER_UNREADABLE",
    "PKI_FORMAT_UNRECOGNISED",
    "PKI_LOCK_FILE_INVALID",
    "PKI_NOT_INSPECTED",
    "PKI_PART_NOT_FOUND",
    "PKI_STORE_FAILED",
];

/// Every failure of this crate. `Display` is the bare code, so a caller that logs the error never
/// writes a path or part text.
#[derive(Debug, Clone, thiserror::Error)]
pub enum InspectError {
    #[error("PKI_FOLDER_UNREADABLE")]
    FolderUnreadable { retryable: bool },
    #[error("PKI_LOCK_FILE_INVALID")]
    LockFileInvalid,
    #[error("PKI_BUNDLE_UNREADABLE")]
    BundleUnreadable,
    #[error("PKI_FORMAT_UNRECOGNISED")]
    FormatUnrecognised,
    #[error("PKI_DECLARATION_TOO_LARGE")]
    DeclarationTooLarge { bytes: u64 },
    #[error("PKI_DECLARATION_INVALID")]
    DeclarationInvalid,
    #[error("PKI_DECLARATION_REFUSED")]
    DeclarationRefused { code: &'static str, retryable: bool },
    #[error("PKI_NOT_INSPECTED")]
    NotInspected,
    #[error("PKI_PART_NOT_FOUND")]
    PartNotFound { part_id: String },
    #[error("PKI_STORE_FAILED")]
    StoreFailed { retryable: bool },
}

impl InspectError {
    /// The registry code; equal to the `Display` text.
    pub fn code(&self) -> &'static str {
        match self {
            InspectError::FolderUnreadable { .. } => "PKI_FOLDER_UNREADABLE",
            InspectError::LockFileInvalid => "PKI_LOCK_FILE_INVALID",
            InspectError::BundleUnreadable => "PKI_BUNDLE_UNREADABLE",
            InspectError::FormatUnrecognised => "PKI_FORMAT_UNRECOGNISED",
            InspectError::DeclarationTooLarge { .. } => "PKI_DECLARATION_TOO_LARGE",
            InspectError::DeclarationInvalid => "PKI_DECLARATION_INVALID",
            InspectError::DeclarationRefused { .. } => "PKI_DECLARATION_REFUSED",
            InspectError::NotInspected => "PKI_NOT_INSPECTED",
            InspectError::PartNotFound { .. } => "PKI_PART_NOT_FOUND",
            InspectError::StoreFailed { .. } => "PKI_STORE_FAILED",
        }
    }

    /// True when the same call may succeed if repeated unchanged later (Error Table).
    pub fn retryable(&self) -> bool {
        match self {
            InspectError::FolderUnreadable { retryable }
            | InspectError::DeclarationRefused { retryable, .. }
            | InspectError::StoreFailed { retryable } => *retryable,
            InspectError::LockFileInvalid
            | InspectError::BundleUnreadable
            | InspectError::FormatUnrecognised
            | InspectError::DeclarationTooLarge { .. }
            | InspectError::DeclarationInvalid
            | InspectError::NotInspected
            | InspectError::PartNotFound { .. } => false,
        }
    }

    /// A file-system failure on the agent folder: transient kinds are retryable, the rest are not.
    pub(crate) fn folder(error: &std::io::Error) -> InspectError {
        use std::io::ErrorKind;
        let retryable = matches!(
            error.kind(),
            ErrorKind::Interrupted | ErrorKind::TimedOut | ErrorKind::WouldBlock
        );
        InspectError::FolderUnreadable { retryable }
    }

    /// A write to the store failed: the store may be writable on the next attempt.
    pub(crate) fn write_failed() -> InspectError {
        InspectError::StoreFailed { retryable: true }
    }

    /// A stored file cannot be used as it is: repeating the call changes nothing.
    pub(crate) fn stored_unusable() -> InspectError {
        InspectError::StoreFailed { retryable: false }
    }
}

impl CodedError for InspectError {
    fn code(&self) -> &'static str {
        InspectError::code(self)
    }

    fn retryable(&self) -> bool {
        InspectError::retryable(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Row: agent folder cannot be listed or read (transient).
    #[test]
    fn folder_unreadable_transient() {
        for kind in [
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::Interrupted,
            std::io::ErrorKind::WouldBlock,
        ] {
            let error = InspectError::folder(&std::io::Error::from(kind));
            assert!(matches!(
                error,
                InspectError::FolderUnreadable { retryable: true }
            ));
            assert!(error.retryable());
            assert_eq!(CodedError::code(&error), "PKI_FOLDER_UNREADABLE");
            assert_eq!(error.to_string(), "PKI_FOLDER_UNREADABLE");
        }
        for kind in [
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied,
        ] {
            let error = InspectError::folder(&std::io::Error::from(kind));
            assert!(!error.retryable());
        }
    }

    #[test]
    fn every_error_has_its_own_code_in_the_list() {
        let errors = [
            InspectError::FolderUnreadable { retryable: false },
            InspectError::LockFileInvalid,
            InspectError::BundleUnreadable,
            InspectError::FormatUnrecognised,
            InspectError::DeclarationTooLarge { bytes: 1 },
            InspectError::DeclarationInvalid,
            InspectError::DeclarationRefused {
                code: "RULE_X",
                retryable: false,
            },
            InspectError::NotInspected,
            InspectError::PartNotFound {
                part_id: String::new(),
            },
            InspectError::StoreFailed { retryable: false },
        ];
        let mut codes: Vec<&str> = errors.iter().map(InspectError::code).collect();
        codes.sort_unstable();
        assert_eq!(codes, ALL_CODES);
        assert!(InspectError::write_failed().retryable());
        assert!(!InspectError::stored_unusable().retryable());
    }
}
