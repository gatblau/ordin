//! `SharePointError`, the one error type of this crate, and the codes it owns (SharePointAdapter.md
//! *Public Interface*, *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_identity::CredError;
use ordin_types::{CodedError, RepairAction, TypesError};

use crate::types::SharePointVersion;

/// Every code the registry gives this crate: the rows of the Error Table. `SRV_BAD_CURSOR` is raised
/// here but owned by `ordin-types`, so it is not listed.
pub const ALL_CODES: &[&str] = &[
    "SP_ACCESS_DENIED",
    "SP_ACTION_KEY_USED",
    "SP_DATASTORE_UNAVAILABLE",
    "SP_LOCATION_INVALID",
    "SP_NAME_EXISTS",
    "SP_NOT_CONFIGURED",
    "SP_NOT_FOUND",
    "SP_NOT_TRACKED",
    "SP_ORIGIN_CHANGED",
    "SP_OUTCOME_UNKNOWN",
    "SP_QUERY_INVALID",
    "SP_RESPONSE_INVALID",
    "SP_SIGN_IN_NEEDED",
    "SP_THROTTLED",
    "SP_TOO_LARGE",
    "SP_UNAVAILABLE",
];

/// Every failure of this crate. `Display` writes the code only, never a provider message, a name from
/// SharePoint, a token or a path.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SharePointError {
    #[error("SP_NOT_CONFIGURED")]
    NotConfigured,
    #[error("SP_SIGN_IN_NEEDED")]
    SignInNeeded,
    #[error("SP_ACCESS_DENIED")]
    AccessDenied,
    #[error("SP_NOT_FOUND")]
    NotFound,
    #[error("SP_LOCATION_INVALID")]
    LocationInvalid,
    #[error("SP_QUERY_INVALID")]
    QueryInvalid,
    #[error("SP_ORIGIN_CHANGED")]
    OriginChanged { current: SharePointVersion },
    #[error("SP_NAME_EXISTS")]
    NameExists,
    #[error("SP_TOO_LARGE")]
    TooLarge,
    #[error("SP_THROTTLED")]
    Throttled { retry_after_s: u32 },
    #[error("SP_UNAVAILABLE")]
    Unavailable,
    #[error("SP_OUTCOME_UNKNOWN")]
    OutcomeUnknown,
    #[error("SP_RESPONSE_INVALID")]
    ResponseInvalid,
    #[error("SP_NOT_TRACKED")]
    NotTracked,
    #[error("SP_DATASTORE_UNAVAILABLE")]
    DatastoreUnavailable,
    #[error("SP_ACTION_KEY_USED")]
    ActionKeyUsed,
    #[error("SRV_BAD_CURSOR")]
    BadCursor,
}

impl CodedError for SharePointError {
    fn code(&self) -> &'static str {
        match self {
            SharePointError::NotConfigured => "SP_NOT_CONFIGURED",
            SharePointError::SignInNeeded => "SP_SIGN_IN_NEEDED",
            SharePointError::AccessDenied => "SP_ACCESS_DENIED",
            SharePointError::NotFound => "SP_NOT_FOUND",
            SharePointError::LocationInvalid => "SP_LOCATION_INVALID",
            SharePointError::QueryInvalid => "SP_QUERY_INVALID",
            SharePointError::OriginChanged { .. } => "SP_ORIGIN_CHANGED",
            SharePointError::NameExists => "SP_NAME_EXISTS",
            SharePointError::TooLarge => "SP_TOO_LARGE",
            SharePointError::Throttled { .. } => "SP_THROTTLED",
            SharePointError::Unavailable => "SP_UNAVAILABLE",
            SharePointError::OutcomeUnknown => "SP_OUTCOME_UNKNOWN",
            SharePointError::ResponseInvalid => "SP_RESPONSE_INVALID",
            SharePointError::NotTracked => "SP_NOT_TRACKED",
            SharePointError::DatastoreUnavailable => "SP_DATASTORE_UNAVAILABLE",
            SharePointError::ActionKeyUsed => "SP_ACTION_KEY_USED",
            SharePointError::BadCursor => "SRV_BAD_CURSOR",
        }
    }

    /// The rows the Error Table marks `Err(retryable)`.
    fn retryable(&self) -> bool {
        matches!(
            self,
            SharePointError::Throttled { .. }
                | SharePointError::Unavailable
                | SharePointError::DatastoreUnavailable
        )
    }

    /// The *Response Body* `action` of the Error Table: `sign_in_again`, `try_again` or none.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            SharePointError::NotConfigured | SharePointError::ResponseInvalid => None,
            SharePointError::SignInNeeded => Some(RepairAction::SignInAgain),
            SharePointError::Throttled { .. }
            | SharePointError::Unavailable
            | SharePointError::DatastoreUnavailable
            | SharePointError::BadCursor => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

impl From<TypesError> for SharePointError {
    /// The only `TypesError` this crate meets is the cursor's (`Cursor::decode`).
    fn from(_: TypesError) -> Self {
        SharePointError::BadCursor
    }
}

impl From<sqlx::Error> for SharePointError {
    fn from(_: sqlx::Error) -> Self {
        SharePointError::DatastoreUnavailable
    }
}

/// Internal Logic 1.1: Identity's code, as this adapter's.
// spec: ordin-desktop/components/SharePointAdapter#C-14
pub(crate) fn from_cred(error: &CredError) -> SharePointError {
    match error.code() {
        "CRED_SIGN_IN_REQUIRED"
        | "CRED_SCOPE_MISSING"
        | "CRED_KEYCHAIN_MISSING"
        | "CRED_REFRESH_REFUSED"
        | "CRED_AMBIGUOUS" => SharePointError::SignInNeeded,
        "CRED_PROVIDER_NOT_CONFIGURED" | "CRED_PROVIDER_REFUSED" => SharePointError::NotConfigured,
        _ if error.retryable() => SharePointError::Unavailable,
        _ => SharePointError::SignInNeeded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permanent(code: &'static str) -> CredError {
        CredError::Permanent { code }
    }

    // Error Table row: the company has not registered ordin with Microsoft.
    #[test]
    fn not_configured() {
        for code in ["CRED_PROVIDER_NOT_CONFIGURED", "CRED_PROVIDER_REFUSED"] {
            let error = from_cred(&permanent(code));
            assert_eq!(error.code(), "SP_NOT_CONFIGURED");
            assert!(!error.retryable());
            assert_eq!(error.repair(), None);
        }
    }

    #[test]
    fn sign_in_codes_map_to_sign_in_needed() {
        for code in [
            "CRED_SIGN_IN_REQUIRED",
            "CRED_SCOPE_MISSING",
            "CRED_KEYCHAIN_MISSING",
            "CRED_REFRESH_REFUSED",
            "CRED_AMBIGUOUS",
            "CRED_NOT_FOUND",
        ] {
            assert_eq!(
                from_cred(&permanent(code)).code(),
                "SP_SIGN_IN_NEEDED",
                "{code}"
            );
        }
    }

    #[test]
    fn retryable_identity_codes_map_to_unavailable() {
        for code in ["CRED_PROVIDER_UNREACHABLE", "CRED_KEYCHAIN_UNAVAILABLE"] {
            let error = from_cred(&CredError::Retryable { code });
            assert_eq!(error.code(), "SP_UNAVAILABLE", "{code}");
            assert!(error.retryable());
        }
    }

    #[test]
    fn cursor_and_sql_errors_convert() {
        assert_eq!(
            SharePointError::from(TypesError::BadCursor).code(),
            "SRV_BAD_CURSOR"
        );
        assert_eq!(
            SharePointError::from(sqlx::Error::PoolClosed).code(),
            "SP_DATASTORE_UNAVAILABLE"
        );
    }
}
