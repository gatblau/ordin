//! Compiles the build's own brand pack into the crate (Brand.md *Internal Logic* A2; *Names chosen
//! here*: `build-brand/`). Before it does, it runs checks C2–C5 over the pack; a build pack that
//! fails stops the build.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[path = "src/check.rs"]
mod check;

fn fail(message: &str) -> ! {
    panic!("build-brand: {message}");
}

fn text<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| fail(&format!("`{key}` is missing or not text")))
}

fn text_map(value: &serde_json::Value, key: &str) -> BTreeMap<String, String> {
    let Some(object) = value.get(key).and_then(serde_json::Value::as_object) else {
        fail(&format!("`{key}` is missing or not an object"));
    };
    object
        .iter()
        .map(|(k, v)| {
            let v = v
                .as_str()
                .unwrap_or_else(|| fail(&format!("`{key}.{k}` is not text")));
            (k.clone(), v.to_owned())
        })
        .collect()
}

/// Reads one asset named by a `file:` URL relative to the folder, checks it (C2) and records it.
fn asset(
    folder: &Path,
    slot: check::AssetSlot,
    asset: &serde_json::Value,
    files: &mut Vec<(String, PathBuf)>,
) {
    let url = text(asset, "url");
    let Some(name) = url.strip_prefix("file:") else {
        fail(&format!("asset url `{url}` is not a `file:` path"));
    };
    if name.is_empty() || name.contains("..") || name.starts_with('/') {
        fail(&format!(
            "asset path `{name}` leaves the build-brand folder"
        ));
    }
    let path = folder.join(name);
    let body = std::fs::read(&path).unwrap_or_else(|e| fail(&format!("{name}: {e}")));
    if let Err(refusal) = check::check_asset(
        slot,
        text(asset, "sha256"),
        text(asset, "media_type"),
        &body,
    ) {
        fail(&format!("{}: {}", refusal.kind.code(), refusal.reason));
    }
    println!("cargo:rerun-if-changed={}", path.display());
    files.push((name.to_owned(), path));
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let folder = manifest.join("build-brand");
    let json_path = folder.join("brand.json");
    println!("cargo:rerun-if-changed={}", folder.display());
    println!("cargo:rerun-if-changed={}", json_path.display());
    println!("cargo:rerun-if-changed=src/check.rs");

    let raw =
        std::fs::read_to_string(&json_path).unwrap_or_else(|e| fail(&format!("brand.json: {e}")));
    let section: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| fail(&format!("brand.json: {e}")));

    let checked = check::check_identifier(text(&section, "brand_identifier"))
        .and_then(|()| check::check_version(text(&section, "version")));
    if let Err(refusal) = checked {
        fail(&format!("{}: {}", refusal.kind.code(), refusal.reason));
    }

    // C2.
    let mut files = Vec::new();
    let themed = |key: &str, theme: &str| -> serde_json::Value {
        section
            .get(key)
            .and_then(|v| v.get(theme))
            .cloned()
            .unwrap_or_else(|| fail(&format!("`{key}.{theme}` is missing")))
    };
    asset(
        &folder,
        check::AssetSlot::LogoLight,
        &themed("logos", "light"),
        &mut files,
    );
    asset(
        &folder,
        check::AssetSlot::LogoDark,
        &themed("logos", "dark"),
        &mut files,
    );
    asset(
        &folder,
        check::AssetSlot::IconSetLight,
        &themed("icon_set", "light"),
        &mut files,
    );
    asset(
        &folder,
        check::AssetSlot::IconSetDark,
        &themed("icon_set", "dark"),
        &mut files,
    );
    let fonts = section
        .get("fonts")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Err(refusal) = check::check_font_count(fonts.len()) {
        fail(&format!("{}: {}", refusal.kind.code(), refusal.reason));
    }
    for (index, font) in fonts.iter().enumerate() {
        asset(&folder, check::AssetSlot::Font(index), font, &mut files);
    }

    // C3.
    let Some(strings) = section
        .get("strings")
        .and_then(serde_json::Value::as_object)
    else {
        fail("`strings` is missing");
    };
    let entries: BTreeMap<String, check::EntryInput> = strings
        .iter()
        .map(|(locale, entry)| {
            (
                locale.clone(),
                check::EntryInput {
                    product_name: text(entry, "product_name").to_owned(),
                    coordinator_name: text(entry, "coordinator_name").to_owned(),
                    about_text: text(entry, "about_text").to_owned(),
                },
            )
        })
        .collect();
    let legal = text_map(&section, "legal_text");
    // C4, C5.
    let tokens = text_map(&section, "design_tokens");
    let checked = check::merge_strings(&entries, &legal)
        .map(|_| ())
        .and_then(|()| check::check_tokens(&tokens))
        .and_then(|()| check::check_contrast(&tokens));
    if let Err(refusal) = checked {
        fail(&format!("{}: {}", refusal.kind.code(), refusal.reason));
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "/// `build-brand/brand.json`, checked by build.rs.\npub(crate) const BRAND_JSON: &str = include_str!({:?});",
        json_path.display().to_string()
    );
    out.push_str("/// Each `file:` asset of the build pack: (path in the folder, bytes).\n");
    out.push_str("pub(crate) const ASSET_FILES: &[(&str, &[u8])] = &[\n");
    for (name, path) in &files {
        let _ = writeln!(
            out,
            "    ({name:?}, include_bytes!({:?})),",
            path.display().to_string()
        );
    }
    out.push_str("];\n");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out_dir.join("build_brand.rs"), out)
        .unwrap_or_else(|e| fail(&format!("write build_brand.rs: {e}")));
}
