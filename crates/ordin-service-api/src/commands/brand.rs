//! Brand — restated from `Brand.md` (SA-RECON-02). Desktop only; the screens read the active pack and
//! its asset bytes through these two commands and restyle from design tokens only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::identity::{BrandPackSource, BrandStrings};
use ordin_types::time::Timestamp;

/// One asset of the active pack. Also an input, so unknown fields are refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssetRef {
    LogoLight,
    LogoDark,
    IconSetLight,
    IconSetDark,
    Font { index: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AssetBytes {
    pub media_type: String,
    /// Standard base64 of the file.
    pub base64: String,
}

/// `brand.get`: the active pack, its strings for the active locale and the newest refusal.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BrandGetResult {
    pub brand_identifier: String,
    pub version: String,
    pub source: BrandPackSource,
    pub strings: BrandStrings,
    /// Token name → value.
    pub design_tokens: BTreeMap<String, String>,
    pub links: BrandGetResultLinks,
    pub font_count: u8,
    /// `null` when no newer pack was refused.
    pub refusal: Option<BrandGetResultRefusal>,
}

/// The `links` object of `brand.get` (named mechanically after its result, SA-RECON-02).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BrandGetResultLinks {
    pub help_url: String,
    pub support_url: String,
    pub privacy_url: String,
}

/// The `refusal` object of `brand.get` (named mechanically after its result, SA-RECON-02).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BrandGetResultRefusal {
    pub brand_identifier: String,
    pub version: String,
    pub reason: String,
    pub received_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandGetInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandGetAssetInput {
    pub asset: AssetRef,
}

impl CommandInput for BrandGetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

/// A font index beyond the pack's fonts is Brand's own refusal (`BRAND_ASSET_UNKNOWN`).
impl CommandInput for BrandGetAssetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_refs() {
        let mut input: BrandGetAssetInput =
            serde_json::from_str(r#"{"asset":{"kind":"font","index":1}}"#).unwrap();
        assert_eq!(input.asset, AssetRef::Font { index: 1 });
        assert!(input.check().is_ok());
        assert!(BrandGetInput {}.check().is_ok());
        assert!(serde_json::from_str::<BrandGetAssetInput>(
            r#"{"asset":{"kind":"font","index":1,"x":1}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<BrandGetAssetInput>(
            r#"{"asset":{"kind":"logo_light"},"x":1}"#
        )
        .is_err());
        assert!(
            serde_json::from_str::<BrandGetAssetInput>(r#"{"asset":{"kind":"banner"}}"#).is_err()
        );
    }

    #[test]
    fn get_result_shape() {
        let result: BrandGetResult = serde_json::from_value(serde_json::json!({
            "brand_identifier":"acme-assist","version":"2.1.0","source":"organisation_profile",
            "strings":{"locale":"en-GB","product_name":"Acme Assist","coordinator_name":"Ada",
                       "about_text":"Acme Assist helps…","legal_text":"© 2026 Acme Ltd."},
            "design_tokens":{"color.text.light":"#1A1A1A"},
            "links":{"help_url":"https://help.acme.example","support_url":"https://support.acme.example",
                     "privacy_url":"https://acme.example/privacy"},
            "font_count":2,"refusal":null
        }))
        .unwrap();
        assert_eq!(result.font_count, 2);
        assert_eq!(
            serde_json::to_value(&result).unwrap()["refusal"],
            serde_json::Value::Null
        );
    }
}
