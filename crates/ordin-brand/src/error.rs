//! Brand's one error type and the codes it owns (Brand.md *Error Table*; ErrorHandling.md
//! *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

use crate::check::RefusalKind;

/// Every code the registry gives this crate (its `BRAND_` rows).
///
/// Two are owned here but never returned as a [`BrandError`]: `BRAND_SUPERSEDED` is only stored and
/// logged on a `received` row replaced before it was checked, and `BRAND_REFUSAL_REASON_MISMATCH` is
/// a defect the `brand_packs` CHECK prevents.
pub const ALL_CODES: &[&str] = &[
    "BRAND_ASSET_FETCH_FAILED",
    "BRAND_ASSET_HASH_MISMATCH",
    "BRAND_ASSET_INVALID",
    "BRAND_ASSET_UNKNOWN",
    "BRAND_CONTRAST_FAILED",
    "BRAND_FIXED_TOKEN",
    "BRAND_ID_INVALID",
    "BRAND_LOCALE_DUPLICATE",
    "BRAND_REFUSAL_REASON_MISMATCH",
    "BRAND_STORE_UNAVAILABLE",
    "BRAND_STRINGS_MISSING",
    "BRAND_STRING_INVALID",
    "BRAND_SUPERSEDED",
    "BRAND_TOKENS_INVALID",
    "BRAND_URL_INVALID",
    "BRAND_VERSION_INVALID",
];

/// `BRAND_SUPERSEDED`: a `received` row replaced by a newer pack before it was checked.
pub(crate) const SUPERSEDED: &str = "BRAND_SUPERSEDED";

/// Every failure of this crate. `Display` never carries a URL, a path or brand text.
#[derive(Debug, thiserror::Error)]
pub enum BrandError {
    #[error("asset could not be fetched")]
    AssetFetchFailed, // BRAND_ASSET_FETCH_FAILED, retryable
    #[error("asset hash mismatch")]
    AssetHashMismatch, // BRAND_ASSET_HASH_MISMATCH
    #[error("asset not acceptable")]
    AssetInvalid, // BRAND_ASSET_INVALID
    #[error("unknown asset")]
    AssetUnknown, // BRAND_ASSET_UNKNOWN
    #[error("store unavailable")]
    StoreUnavailable(#[source] sqlx::Error), // BRAND_STORE_UNAVAILABLE, retryable
    #[error("pack refused: {0}")]
    Refused(RefusalKind), // see Error Table
}

impl BrandError {
    /// A stored row or file that no longer reads: the same retryable condition as an unreachable
    /// store.
    pub(crate) fn corrupt(what: &'static str) -> BrandError {
        BrandError::StoreUnavailable(sqlx::Error::Decode(what.into()))
    }
}

impl From<sqlx::Error> for BrandError {
    fn from(error: sqlx::Error) -> Self {
        BrandError::StoreUnavailable(error)
    }
}

/// The one error contract of module errors (2C `CodedError`).
impl CodedError for BrandError {
    fn code(&self) -> &'static str {
        match self {
            BrandError::AssetFetchFailed => "BRAND_ASSET_FETCH_FAILED",
            BrandError::AssetHashMismatch => "BRAND_ASSET_HASH_MISMATCH",
            BrandError::AssetInvalid => "BRAND_ASSET_INVALID",
            BrandError::AssetUnknown => "BRAND_ASSET_UNKNOWN",
            BrandError::StoreUnavailable(_) => "BRAND_STORE_UNAVAILABLE",
            BrandError::Refused(kind) => kind.code(),
        }
    }

    /// `true` for `AssetFetchFailed` and `StoreUnavailable` only.
    fn retryable(&self) -> bool {
        matches!(
            self,
            BrandError::AssetFetchFailed | BrandError::StoreUnavailable(_)
        )
    }

    /// `TryAgain` for the two retryable errors, `None` otherwise.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            BrandError::AssetFetchFailed | BrandError::StoreUnavailable(_) => {
                Some(RepairAction::TryAgain)
            }
            _ => None,
        }
    }
}
