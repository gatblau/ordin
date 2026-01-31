//! The one keyset page form (SA-PAGE-01; Pagination.md; 2C domain *Paging*, C-15).

use serde::{Deserialize, Serialize};

use crate::encoding::b64url_encode;
use crate::TypesError;

pub const PAGE_SIZE: usize = 100;
pub const CURSOR_MAX_CHARS: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Always serialised; `null` on the last page (the one `Option` not skipped when `None`).
    pub next_cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PageRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

/// Opaque: base64url of the JSON form of the last row's sort key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct Cursor(String);

impl Cursor {
    /// base64url of the JSON form of the last row's sort key.
    ///
    /// A key whose JSON form cannot be written (a map with non-string keys) encodes as the empty
    /// cursor, which `decode` refuses; a sort key is a scalar or a tuple, so this does not arise.
    pub fn encode<K: Serialize>(key: &K) -> Cursor {
        let json = serde_json::to_vec(key).unwrap_or_default();
        Cursor(b64url_encode(&json))
    }

    /// Refuses more than 1,024 characters, bad base64url, or JSON that is not a `K` (`SRV_BAD_CURSOR`).
    ///
    /// The cursor carries no signature: it holds only a sort key the caller already saw, and every
    /// query applies the caller's own scope, so forging one reveals nothing (Internal Logic 8).
    pub fn decode<K: serde::de::DeserializeOwned>(&self) -> Result<K, TypesError> {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine as _;

        if self.0.chars().count() > CURSOR_MAX_CHARS {
            return Err(TypesError::BadCursor);
        }
        let json = URL_SAFE_NO_PAD
            .decode(&self.0)
            .map_err(|_| TypesError::BadCursor)?;
        serde_json::from_slice::<K>(&json).map_err(|_| TypesError::BadCursor)
    }
}
