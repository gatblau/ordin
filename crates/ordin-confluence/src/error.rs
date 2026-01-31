//! `ConfluenceError`, the one error type of this crate, and the codes it owns (ConfluenceAdapter.md
//! *Public Interface*, *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_identity::CredError;
use ordin_types::{CodedError, RepairAction, TypesError};

use crate::types::RemoteVersion;

/// Every code the registry gives this crate: the rows of the Error Table. `SRV_BAD_CURSOR` is raised
/// here but owned by `ordin-types`, so it is not listed.
pub const ALL_CODES: &[&str] = &[
    "CONF_ACCESS_DENIED",
    "CONF_ACTION_KEY_USED",
    "CONF_APPROVAL_FAILED",
    "CONF_CONVERSION_FAILED",
    "CONF_DATASTORE_UNAVAILABLE",
    "CONF_FILES_FAILED",
    "CONF_LINK_NOT_ACTIVE",
    "CONF_LOCATION_INVALID",
    "CONF_NOT_CONFIGURED",
    "CONF_NOT_FOUND",
    "CONF_NOT_TRACKED",
    "CONF_ORIGIN_CHANGED",
    "CONF_OUTCOME_UNKNOWN",
    "CONF_QUERY_INVALID",
    "CONF_RESPONSE_INVALID",
    "CONF_SIGN_IN_NEEDED",
    "CONF_SITE_NOT_GRANTED",
    "CONF_THROTTLED",
    "CONF_TITLE_EXISTS",
    "CONF_TOO_LARGE",
    "CONF_UNAVAILABLE",
];

/// Every failure of this crate. `Display` writes the code only, never a Confluence message, a title,
/// a token or a path.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfluenceError {
    #[error("CONF_NOT_CONFIGURED")]
    NotConfigured,
    #[error("CONF_SIGN_IN_NEEDED")]
    SignInNeeded,
    #[error("CONF_SITE_NOT_GRANTED")]
    SiteNotGranted,
    #[error("CONF_ACCESS_DENIED")]
    AccessDenied,
    #[error("CONF_NOT_FOUND")]
    NotFound,
    #[error("CONF_LOCATION_INVALID")]
    LocationInvalid,
    #[error("CONF_QUERY_INVALID")]
    QueryInvalid,
    #[error("CONF_ORIGIN_CHANGED")]
    OriginChanged { current: RemoteVersion },
    #[error("CONF_TITLE_EXISTS")]
    TitleExists,
    #[error("CONF_TOO_LARGE")]
    TooLarge,
    #[error("CONF_THROTTLED")]
    Throttled,
    #[error("CONF_UNAVAILABLE")]
    Unavailable,
    #[error("CONF_OUTCOME_UNKNOWN")]
    OutcomeUnknown,
    #[error("CONF_RESPONSE_INVALID")]
    ResponseInvalid,
    #[error("CONF_CONVERSION_FAILED")]
    ConversionFailed,
    #[error("CONF_LINK_NOT_ACTIVE")]
    LinkNotActive,
    #[error("CONF_NOT_TRACKED")]
    NotTracked,
    #[error("CONF_ACTION_KEY_USED")]
    ActionKeyUsed,
    #[error("CONF_APPROVAL_FAILED")]
    ApprovalFailed,
    #[error("CONF_FILES_FAILED")]
    FilesFailed,
    #[error("CONF_DATASTORE_UNAVAILABLE")]
    DatastoreUnavailable,
    // Internal Logic 1.4 refuses a cursor that does not decode to a `/wiki/` path with
    // `SRV_BAD_CURSOR`, as the SharePoint Adapter's enum does.
    #[error("SRV_BAD_CURSOR")]
    BadCursor,
}

impl ConfluenceError {
    /// The registry code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            ConfluenceError::NotConfigured => "CONF_NOT_CONFIGURED",
            ConfluenceError::SignInNeeded => "CONF_SIGN_IN_NEEDED",
            ConfluenceError::SiteNotGranted => "CONF_SITE_NOT_GRANTED",
            ConfluenceError::AccessDenied => "CONF_ACCESS_DENIED",
            ConfluenceError::NotFound => "CONF_NOT_FOUND",
            ConfluenceError::LocationInvalid => "CONF_LOCATION_INVALID",
            ConfluenceError::QueryInvalid => "CONF_QUERY_INVALID",
            ConfluenceError::OriginChanged { .. } => "CONF_ORIGIN_CHANGED",
            ConfluenceError::TitleExists => "CONF_TITLE_EXISTS",
            ConfluenceError::TooLarge => "CONF_TOO_LARGE",
            ConfluenceError::Throttled => "CONF_THROTTLED",
            ConfluenceError::Unavailable => "CONF_UNAVAILABLE",
            ConfluenceError::OutcomeUnknown => "CONF_OUTCOME_UNKNOWN",
            ConfluenceError::ResponseInvalid => "CONF_RESPONSE_INVALID",
            ConfluenceError::ConversionFailed => "CONF_CONVERSION_FAILED",
            ConfluenceError::LinkNotActive => "CONF_LINK_NOT_ACTIVE",
            ConfluenceError::NotTracked => "CONF_NOT_TRACKED",
            ConfluenceError::ActionKeyUsed => "CONF_ACTION_KEY_USED",
            ConfluenceError::ApprovalFailed => "CONF_APPROVAL_FAILED",
            ConfluenceError::FilesFailed => "CONF_FILES_FAILED",
            ConfluenceError::DatastoreUnavailable => "CONF_DATASTORE_UNAVAILABLE",
            ConfluenceError::BadCursor => "SRV_BAD_CURSOR",
        }
    }

    /// The rows the Error Table marks `Err(retryable)`.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            ConfluenceError::Throttled
                | ConfluenceError::Unavailable
                | ConfluenceError::ApprovalFailed
                | ConfluenceError::FilesFailed
                | ConfluenceError::DatastoreUnavailable
        )
    }
}

impl CodedError for ConfluenceError {
    fn code(&self) -> &'static str {
        ConfluenceError::code(self)
    }

    fn retryable(&self) -> bool {
        ConfluenceError::retryable(self)
    }

    /// The *Response Body* `action` of the Error Table: `sign_in_again`, `try_again` or none.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            ConfluenceError::NotConfigured | ConfluenceError::ResponseInvalid => None,
            ConfluenceError::SignInNeeded | ConfluenceError::SiteNotGranted => {
                Some(RepairAction::SignInAgain)
            }
            ConfluenceError::Throttled
            | ConfluenceError::Unavailable
            | ConfluenceError::ApprovalFailed
            | ConfluenceError::FilesFailed
            | ConfluenceError::DatastoreUnavailable
            | ConfluenceError::BadCursor => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

impl From<TypesError> for ConfluenceError {
    /// The only `TypesError` this crate meets is the cursor's (`Cursor::decode`).
    fn from(_: TypesError) -> Self {
        ConfluenceError::BadCursor
    }
}

impl From<sqlx::Error> for ConfluenceError {
    fn from(_: sqlx::Error) -> Self {
        ConfluenceError::DatastoreUnavailable
    }
}

/// Internal Logic 1.1: Identity's code, as this adapter's.
// spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-07
pub(crate) fn from_cred(error: &CredError) -> ConfluenceError {
    match CodedError::code(error) {
        "CRED_SIGN_IN_REQUIRED"
        | "CRED_SCOPE_MISSING"
        | "CRED_PROVIDER_REFUSED"
        | "CRED_KEYCHAIN_MISSING" => ConfluenceError::SignInNeeded,
        "CRED_PROVIDER_NOT_CONFIGURED" => ConfluenceError::NotConfigured,
        _ if CodedError::retryable(error) => ConfluenceError::Unavailable,
        _ => ConfluenceError::SignInNeeded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permanent(code: &'static str) -> CredError {
        CredError::Permanent { code }
    }

    // Error Table row: Identity answers `CRED_PROVIDER_NOT_CONFIGURED`, which a stored credential cannot
    // produce through the service.
    #[test]
    fn provider_not_configured_is_not_configured() {
        let error = from_cred(&permanent("CRED_PROVIDER_NOT_CONFIGURED"));
        assert_eq!(error.code(), "CONF_NOT_CONFIGURED");
        assert!(!error.retryable());
        assert_eq!(error.repair(), None);
    }

    #[test]
    fn sign_in_codes_map_to_sign_in_needed() {
        for code in [
            "CRED_SIGN_IN_REQUIRED",
            "CRED_SCOPE_MISSING",
            "CRED_PROVIDER_REFUSED",
            "CRED_KEYCHAIN_MISSING",
            "CRED_AMBIGUOUS",
            "CRED_NOT_FOUND",
        ] {
            assert_eq!(
                from_cred(&permanent(code)).code(),
                "CONF_SIGN_IN_NEEDED",
                "{code}"
            );
        }
    }

    #[test]
    fn retryable_identity_codes_map_to_unavailable() {
        for code in ["CRED_PROVIDER_UNREACHABLE", "CRED_KEYCHAIN_UNAVAILABLE"] {
            let error = from_cred(&CredError::Retryable { code });
            assert_eq!(error.code(), "CONF_UNAVAILABLE", "{code}");
            assert!(error.retryable());
        }
    }

    #[test]
    fn cursor_and_sql_errors_convert() {
        assert_eq!(
            ConfluenceError::from(TypesError::BadCursor).code(),
            "SRV_BAD_CURSOR"
        );
        assert_eq!(
            ConfluenceError::from(sqlx::Error::PoolClosed).code(),
            "CONF_DATASTORE_UNAVAILABLE"
        );
    }
}
