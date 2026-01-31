//! The configuration and the asset types of the Public Interface (Brand.md).

use serde::{Deserialize, Serialize};

/// What the desktop program hands Brand at start.
pub struct BrandConfig {
    /// The Journal's `Datastore` (D4, R20); the desktop is SQLite only and this crate uses `datastore.sqlite()`.
    // spec: ordin-desktop/components/Brand#SA-RECON-01
    pub datastore: ordin_journal::Datastore,
    /// `<application data root>`; assets go under `brand/`.
    pub data_root: std::path::PathBuf,
    /// Shared outbound client (SA-HTTP-02), built by the program.
    // spec: ordin-desktop/components/Brand#SA-HTTP-02
    pub http: reqwest::Client,
}

/// One asset of the active pack, as `brand.get_asset` names it.
// spec: ordin-desktop/components/Brand#SA-RECON-02
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetRef {
    LogoLight,
    LogoDark,
    IconSetLight,
    IconSetDark,
    Font { index: u8 },
}

/// The bytes of one asset, for the WebView (which makes no network call).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AssetBytes {
    pub media_type: String,
    /// Standard base64 of the file.
    pub base64: String,
}
