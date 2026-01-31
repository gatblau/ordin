//! Builds `CraftProfile` from a profile snapshot (CraftToolkit.md, after *Other components' calls*).

use std::collections::BTreeMap;

use ordin_organisation_profile::{CraftApp, CraftAsset, ProfilePlatform, ProfileSnapshot};

use crate::model::{CraftAppPin, CraftPinnedAsset, CraftPlatform, CraftProfile};

/// Unit tests reach the `wiremock` stand-in over plain HTTP on loopback; every build that ships
/// accepts `https` addresses only.
const ALLOW_LOOPBACK_HTTP: bool = cfg!(test);

fn platform_of(platform: ProfilePlatform) -> CraftPlatform {
    match platform {
        ProfilePlatform::MacosArm64 => CraftPlatform::MacosArm64,
        ProfilePlatform::MacosX86_64 => CraftPlatform::MacosX86_64,
        ProfilePlatform::WindowsX86_64 => CraftPlatform::WindowsX86_64,
        ProfilePlatform::LinuxX86_64 => CraftPlatform::LinuxX86_64,
    }
}

// spec: ordin-desktop/components/CraftToolkit#SA-RECON-06
pub(crate) fn build(snapshot: &ProfileSnapshot) -> CraftProfile {
    let craft = &snapshot.document.craft;
    CraftProfile {
        profile_version: snapshot.document.profile_version.clone(),
        apps: craft.apps.iter().map(pin_of).collect(),
        file_types: craft.file_types.clone(),
    }
}

// spec: ordin-desktop/components/CraftToolkit#SA-CLOSE-01
fn pin_of(app: &CraftApp) -> CraftAppPin {
    let mut platforms = BTreeMap::new();
    for (platform, asset) in &app.platforms {
        let platform = platform_of(*platform);
        match asset_of(asset) {
            Some(pinned) => {
                platforms.insert(platform, pinned);
            }
            None => tracing::warn!(
                component = "craft",
                app = %app.app_id,
                platform = ?platform,
                "craft_pin_invalid"
            ),
        }
    }
    CraftAppPin {
        name: app.app_id.clone(),
        version: app.version.clone(),
        platforms,
        allowed_tools: app.allowed_tools.clone(),
    }
}

/// `None` when the address is not an `https` URL, the hash is not 64 hex characters, or the file
/// name holds `/`, `\` or `..`.
fn asset_of(asset: &CraftAsset) -> Option<CraftPinnedAsset> {
    let asset_url = url::Url::parse(&asset.asset_url).ok()?;
    let loopback = ALLOW_LOOPBACK_HTTP && asset_url.host_str() == Some("127.0.0.1");
    if asset_url.scheme() != "https" && !loopback {
        return None;
    }
    let hash = &asset.sha256.0;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let name = &asset.file_name;
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    Some(CraftPinnedAsset {
        asset_url,
        file_name: name.clone(),
        sha256: asset.sha256.clone(),
    })
}

/// The app's name for a person: "photocraft" reads "PhotoCraft".
/// Derived from `app_id`: no definition or profile field carries a display name.
pub(crate) fn display_name(app: &str) -> String {
    let (stem, suffix) = match app.strip_suffix("craft") {
        Some(stem) if !stem.is_empty() => (stem, "Craft"),
        _ => (app, ""),
    };
    let mut chars = stem.chars();
    let first = chars.next().map(|c| c.to_ascii_uppercase());
    first
        .into_iter()
        .chain(chars)
        .chain(suffix.chars())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::num::Sha256Hex;

    fn asset(url: &str, sha: &str, name: &str) -> CraftAsset {
        CraftAsset {
            asset_url: url.to_owned(),
            sha256: Sha256Hex(sha.to_owned()),
            file_name: name.to_owned(),
        }
    }

    #[test]
    fn a_bad_pin_entry_is_dropped() {
        let good = "a".repeat(64);
        let url = "https://mirror.acme.co.uk/p.zip";
        assert!(asset_of(&asset(url, &good, "p.zip")).is_some());
        for bad in [
            asset("http://mirror.acme.co.uk/p.zip", &good, "p.zip"),
            asset("not a url", &good, "p.zip"),
            asset(url, "abc", "p.zip"),
            asset(url, &"z".repeat(64), "p.zip"),
            asset(url, &good, "a/p.zip"),
            asset(url, &good, "a\\p.zip"),
            asset(url, &good, "..p.zip"),
        ] {
            assert!(asset_of(&bad).is_none());
        }
    }

    #[test]
    fn display_names() {
        assert_eq!(display_name("photocraft"), "PhotoCraft");
        assert_eq!(display_name("gridcraft"), "GridCraft");
        assert_eq!(display_name("notes"), "Notes");
        assert_eq!(display_name("craft"), "Craft");
    }
}
