//! X1: one test per row of Brand.md *Error Table*, and X4: the codes this crate owns.

use std::collections::BTreeMap;

use ordin_organisation_profile::BrandSection;
use ordin_types::domain::identity::{BrandPack, BrandPackSource, BrandPackState};
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::time::Timestamp;
use ordin_types::{BrandPackId, CodedError as _, RepairAction};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::support::*;
use crate::{AssetRef, Brand, BrandConfig, BrandError, RefusalKind, ALL_CODES};

/// A change to a section that may name an asset served by the harness.
type HarnessChange = Box<dyn Fn(&Harness, &mut BrandSection)>;
/// A change to a section.
type SectionChange = Box<dyn Fn(&mut BrandSection)>;
/// A change to a section's design tokens.
type TokenChange = Box<dyn Fn(&mut BTreeMap<String, String>)>;

/// Receives `section`, expects a refusal with `code`, and returns the stored row.
async fn refused(h: &Harness, section: &BrandSection, code: &str) -> BrandPack {
    let error = h.brand.receive(section).await.expect_err(code);
    assert_eq!(error.code(), code);
    assert!(!error.retryable(), "{code} is permanent");
    assert_eq!(error.repair(), None);
    let row = h
        .row(&section.brand_identifier, &section.version)
        .await
        .expect("the row");
    assert_eq!(row.state, BrandPackState::Refused, "{code}");
    assert_eq!(h.brand.active().source, BrandPackSource::Build, "{code}");
    row
}

fn reason(row: &BrandPack) -> &str {
    row.refusal_reason.as_deref().expect("a reason")
}

#[tokio::test]
async fn asset_download_fails_after_three_attempts() {
    let h = Harness::new().await;
    for status in [503u16, 429] {
        let name = if status == 503 { FONT_1 } else { FONT_2 };
        Mock::given(method("GET"))
            .and(path(format!("/brand/{name}")))
            .respond_with(ResponseTemplate::new(status))
            .with_priority(1)
            .mount(&h.server)
            .await;
    }
    h.assets.serve(&h.server).await;
    let section = h.section("acme-assist", "2.1.0");

    let error = h.brand.receive(&section).await.expect_err("fetch failed");
    assert!(matches!(error, BrandError::AssetFetchFailed));
    assert_eq!(error.code(), "BRAND_ASSET_FETCH_FAILED");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
    let row = h.row("acme-assist", "2.1.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Received);
    assert_eq!(h.brand.active().source, BrandPackSource::Build);
    let font_requests = h
        .server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path() == format!("/brand/{FONT_1}"))
        .count();
    assert_eq!(font_requests, 3, "three attempts");

    // A network error (nothing listens) is retried and fails the same way.
    let mut unreachable = h.section("acme-assist", "2.1.1");
    unreachable.logos.light.url = "http://127.0.0.1:9/brand/logo-light.svg".to_owned();
    let error = h
        .brand
        .receive(&unreachable)
        .await
        .expect_err("fetch failed");
    assert_eq!(error.code(), "BRAND_ASSET_FETCH_FAILED");
}

#[tokio::test]
async fn asset_body_sha256_differs_from_the_section() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let mut section = h.section("acme-assist", "2.2.0");
    section.icon_set.dark.sha256 = ordin_types::num::Sha256Hex("0".repeat(64));

    let row = refused(&h, &section, "BRAND_ASSET_HASH_MISMATCH").await;
    assert_eq!(
        reason(&row),
        "The dark icon set file does not match the brand's checksum."
    );
    // The bodies accepted before the mismatch were written, then the version folder was deleted.
    assert!(!h.data_root().join("brand/acme-assist/2.2.0").exists());
}

#[tokio::test]
async fn asset_too_large_wrong_type_active_svg_or_http_4xx() {
    let mut h = Harness::new().await;
    let mut big = b"\x89PNG\r\n\x1a\n".to_vec();
    big.resize(2 * 1024 * 1024 + 1, 0);
    h.assets.files.insert("big.png", (big, "image/png"));
    h.assets
        .files
        .insert("page.html", (b"<html></html>".to_vec(), "image/svg+xml"));
    h.assets.files.insert(
        "external.svg",
        (
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><image href=\"https://tracker.example/p.png\"/></svg>"
                .to_vec(),
            "image/svg+xml",
        ),
    );
    h.assets
        .files
        .insert("font.ttf", (b"\x00\x01\x00\x00ttf".to_vec(), "font/gif"));
    h.assets.serve(&h.server).await;
    Mock::given(method("GET"))
        .and(path("/brand/gone.svg"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&h.server)
        .await;

    let cases: Vec<(&str, HarnessChange, &str)> = vec![
        (
            "2.0.1",
            Box::new(|h, s| s.logos.light = h.assets.asset(&h.server, "big.png")),
            "The light logo file isn't an accepted image.",
        ),
        (
            "2.0.2",
            Box::new(|h, s| s.icon_set.light = h.assets.asset(&h.server, "page.html")),
            "The light icon set file isn't an accepted image.",
        ),
        (
            "2.0.3",
            Box::new(|h, s| s.logos.dark = h.assets.asset(&h.server, "external.svg")),
            "The dark logo file isn't an accepted image.",
        ),
        (
            "2.0.4",
            Box::new(|h, s| s.fonts[1] = h.assets.asset(&h.server, "font.ttf")),
            "The font 2 file isn't an accepted font.",
        ),
        (
            "2.0.5",
            Box::new(|h, s| {
                s.icon_set.dark.url = format!("{}/brand/gone.svg", h.server.uri());
            }),
            "The dark icon set file isn't an accepted image.",
        ),
        (
            "2.0.6",
            Box::new(|h, s| {
                let font = s.fonts[0].clone();
                s.fonts = vec![font; 9];
                let _ = h;
            }),
            "The font 9 file isn't an accepted font.",
        ),
        (
            "2.0.7",
            Box::new(|_, s| s.logos.light.url = "ftp://assets.acme.example/logo.svg".to_owned()),
            "The light logo file isn't an accepted image.",
        ),
    ];
    for (version, change, expected) in cases {
        let mut section = h.section("acme-assist", version);
        change(&h, &mut section);
        let row = refused(&h, &section, "BRAND_ASSET_INVALID").await;
        assert_eq!(reason(&row), expected, "{version}");
    }
}

#[tokio::test]
async fn brand_identifier_invalid() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let section = h.section("Acme_Assist", "2.1.0");
    let row = refused(&h, &section, "BRAND_ID_INVALID").await;
    assert_eq!(reason(&row), "The brand identifier isn't valid.");

    // Too long to store: refused without a row.
    let long = h.section(&"a".repeat(65), "2.1.0");
    let error = h.brand.receive(&long).await.expect_err("refused");
    assert_eq!(error.code(), "BRAND_ID_INVALID");
    assert_eq!(h.brand.active().source, BrandPackSource::Build);
}

#[tokio::test]
async fn version_not_major_minor_patch() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let row = refused(
        &h,
        &h.section("acme-assist", "2.1"),
        "BRAND_VERSION_INVALID",
    )
    .await;
    assert_eq!(reason(&row), "The brand version isn't valid.");
}

#[tokio::test]
async fn no_strings_or_a_locale_lacking_legal_text_or_strings() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;

    let mut no_legal = h.section("acme-assist", "2.1.0");
    no_legal.strings.insert(
        "fr-FR".to_owned(),
        entry("Acme Assistant", "Adèle", "Acme Assistant aide…"),
    );
    let row = refused(&h, &no_legal, "BRAND_STRINGS_MISSING").await;
    assert_eq!(reason(&row), "The brand has no legal text for fr-FR.");

    let mut no_strings = h.section("acme-assist", "2.1.1");
    no_strings
        .legal_text
        .insert("de-DE".to_owned(), "© 2026 Acme GmbH.".to_owned());
    let row = refused(&h, &no_strings, "BRAND_STRINGS_MISSING").await;
    assert_eq!(reason(&row), "The brand has no strings for de-DE.");

    let mut empty = h.section("acme-assist", "2.1.2");
    empty.strings.clear();
    empty.legal_text.clear();
    let row = refused(&h, &empty, "BRAND_STRINGS_MISSING").await;
    assert_eq!(reason(&row), "The brand has no strings.");

    let mut not_a_tag = h.section("acme-assist", "2.1.3");
    let entry = not_a_tag.strings["en-GB"].clone();
    not_a_tag.strings.insert("not a tag!".to_owned(), entry);
    refused(&h, &not_a_tag, "BRAND_STRINGS_MISSING").await;
}

#[tokio::test]
async fn two_locales_equal_after_canonicalisation() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let mut section = h.section("acme-assist", "2.1.0");
    let entry = section.strings["en-GB"].clone();
    section.strings.insert("en-gb".to_owned(), entry);
    section
        .legal_text
        .insert("en-gb".to_owned(), "© 2026 Acme Ltd.".to_owned());
    let row = refused(&h, &section, "BRAND_LOCALE_DUPLICATE").await;
    assert_eq!(reason(&row), "The brand lists en-GB twice.");
}

#[tokio::test]
async fn a_brand_string_outside_its_length() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let cases: Vec<(&str, SectionChange, &str)> = vec![
        (
            "2.1.0",
            Box::new(|s| {
                s.strings.get_mut("en-GB").unwrap().product_name = "P".repeat(61);
            }),
            "The product name for en-GB is too long.",
        ),
        (
            "2.1.1",
            Box::new(|s| s.strings.get_mut("en-GB").unwrap().coordinator_name = String::new()),
            "The coordinator name for en-GB is empty.",
        ),
        (
            "2.1.2",
            Box::new(|s| s.strings.get_mut("en-GB").unwrap().about_text = "a".repeat(8001)),
            "The about text for en-GB is too long.",
        ),
        (
            "2.1.3",
            Box::new(|s| {
                s.legal_text.insert("en-GB".to_owned(), "l".repeat(8001));
            }),
            "The legal text for en-GB is too long.",
        ),
    ];
    for (version, change, expected) in cases {
        let mut section = h.section("acme-assist", version);
        change(&mut section);
        let row = refused(&h, &section, "BRAND_STRING_INVALID").await;
        assert_eq!(reason(&row), expected, "{version}");
    }
}

#[tokio::test]
async fn token_name_value_required_colour_or_count_invalid() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let cases: Vec<(&str, TokenChange, String)> = vec![
        (
            "2.1.0",
            Box::new(|t| {
                t.insert("shadow.card".to_owned(), "none".to_owned());
            }),
            "The design token shadow.card isn't allowed.".to_owned(),
        ),
        (
            "2.1.1",
            Box::new(|t| {
                t.insert("space.lg".to_owned(), "2em".to_owned());
            }),
            "The design token space.lg has a value that isn't valid.".to_owned(),
        ),
        (
            "2.1.2",
            Box::new(|t| {
                t.insert("color.text.light".to_owned(), "black".to_owned());
            }),
            "The design token color.text.light has a value that isn't valid.".to_owned(),
        ),
        (
            "2.1.3",
            Box::new(|t| {
                t.insert("font.heading".to_owned(), "Acme; Sans".to_owned());
            }),
            "The design token font.heading has a value that isn't valid.".to_owned(),
        ),
        (
            "2.1.4",
            Box::new(|t| {
                t.remove("color.text.dark");
            }),
            "The design token color.text.dark is missing.".to_owned(),
        ),
        (
            "2.1.5",
            Box::new(|t| {
                for n in 0..500 {
                    t.insert(format!("space.s{n}"), "4px".to_owned());
                }
            }),
            "The brand has more than 500 design tokens.".to_owned(),
        ),
    ];
    for (version, change, expected) in cases {
        let mut section = h.section("acme-assist", version);
        change(&mut section.design_tokens);
        let row = refused(&h, &section, "BRAND_TOKENS_INVALID").await;
        assert_eq!(reason(&row), expected, "{version}");
    }
}

#[tokio::test]
async fn token_sets_a_fixed_safety_colour() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    for (version, name) in [
        ("2.1.0", "color.danger.light"),
        ("2.1.1", "color.waiting.dark"),
        ("2.1.2", "color.run-state.running.light"),
        ("2.1.3", "color.approval.card.dark"),
    ] {
        let mut section = h.section("acme-assist", version);
        section
            .design_tokens
            .insert(name.to_owned(), "#123456".to_owned());
        let row = refused(&h, &section, "BRAND_FIXED_TOKEN").await;
        assert_eq!(
            reason(&row),
            format!("The brand tries to change {name}, which ordin keeps fixed.")
        );
    }
}

#[tokio::test]
async fn a_pair_below_its_contrast_threshold() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let mut section = h.section("acme-assist", "2.1.0");
    section
        .design_tokens
        .insert("color.text.dark".to_owned(), "#636363".to_owned());
    let row = refused(&h, &section, "BRAND_CONTRAST_FAILED").await;
    // Text on background fails at 3.1:1, and text on surface fails too.
    assert_eq!(
        reason(&row),
        "Text colour on background in the dark theme has contrast 3.1:1; at least 4.5:1 is needed, and 1 more."
    );

    let mut one = h.section("acme-assist", "2.1.1");
    one.design_tokens.insert(
        "color.border-control.light".to_owned(),
        "#AAAAAA".to_owned(),
    );
    let row = refused(&h, &one, "BRAND_CONTRAST_FAILED").await;
    assert_eq!(
        reason(&row),
        "Control border colour on background in the light theme has contrast 2.3:1; at least 3:1 is needed."
    );
}

#[tokio::test]
async fn a_link_not_https_or_too_long() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let mut section = h.section("acme-assist", "2.1.0");
    section.links.privacy_url = "http://acme.example/privacy".to_owned();
    let row = refused(&h, &section, "BRAND_URL_INVALID").await;
    assert_eq!(
        reason(&row),
        "The brand's privacy link isn't a secure web address."
    );

    let mut long = h.section("acme-assist", "2.1.1");
    long.links.help_url = format!("https://help.acme.example/{}", "a".repeat(2048));
    let row = refused(&h, &long, "BRAND_URL_INVALID").await;
    assert_eq!(
        reason(&row),
        "The brand's help link isn't a secure web address."
    );
}

/// A `received` row as a fetch interrupted by quit leaves it.
fn received_row(brand: &str, version: &str) -> BrandPack {
    BrandPack {
        id: BrandPackId::new(),
        brand_identifier: brand.to_owned(),
        version: version.to_owned(),
        source: BrandPackSource::OrganisationProfile,
        strings: Vec::new(),
        logo_light_path: String::new(),
        logo_dark_path: String::new(),
        icon_set_light_path: String::new(),
        icon_set_dark_path: String::new(),
        design_tokens: BTreeMap::new(),
        font_file_paths: Vec::new(),
        help_url: String::new(),
        support_url: String::new(),
        privacy_url: String::new(),
        state: BrandPackState::Received,
        refusal_reason: None,
        received_at: Timestamp::now(),
        activated_at: None,
    }
}

#[tokio::test]
async fn a_received_row_replaced_by_a_newer_pack_before_checking() {
    let mut h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let brand = unique("acme");

    // At start: of two `received` rows, the older is superseded.
    let older = received_row(&brand, "1.0.0");
    crate::store::insert(h.pool(), &older).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let newer = received_row(&brand, "1.1.0");
    crate::store::insert(h.pool(), &newer).await.unwrap();
    h.restart().await;
    let row = h.row(&brand, "1.0.0").await.unwrap();
    assert_eq!(row.state, BrandPackState::Refused);
    assert_eq!(
        row.refusal_reason.as_deref(),
        Some("Replaced by a newer brand before it was checked.")
    );
    assert_eq!(
        h.row(&brand, "1.1.0").await.unwrap().state,
        BrandPackState::Received
    );
    let lines = logged("brand.refused", &brand);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].field("code"), "BRAND_SUPERSEDED");

    // A profile naming another pack supersedes the one still `received`.
    h.brand
        .receive(&h.section(&brand, "2.0.0"))
        .await
        .expect("activated");
    assert_eq!(
        h.row(&brand, "1.1.0").await.unwrap().state,
        BrandPackState::Refused
    );
    assert_eq!(h.brand.active().version, "2.0.0");
    assert!(ALL_CODES.contains(&"BRAND_SUPERSEDED"));
}

#[tokio::test]
async fn get_asset_with_a_font_index_out_of_range() {
    let h = Harness::new().await;
    // The build pack has no fonts.
    let error = h
        .brand
        .asset(AssetRef::Font { index: 0 })
        .await
        .expect_err("unknown");
    assert!(matches!(error, BrandError::AssetUnknown));
    assert_eq!(error.code(), "BRAND_ASSET_UNKNOWN");
    assert!(!error.retryable());
    assert_eq!(error.repair(), None);
}

#[tokio::test]
async fn brand_packs_unreadable_or_unwritable() {
    install();
    let dir = Dir::new();
    // A datastore without a SQLite pool (the desktop never has one).
    let pg = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://brand@127.0.0.1:9/none")
        .expect("a lazy pool");
    let error = start_public(ordin_journal::Datastore::Postgres(pg), &dir)
        .await
        .err()
        .expect("no SQLite");
    assert_store_unavailable(&error);

    // A SQLite datastore whose pool is closed.
    let store = open_store(&dir).await;
    store.sqlite().unwrap().close().await;
    let error = start_public(store, &dir).await.err().expect("closed");
    assert_store_unavailable(&error);

    // A SQLite datastore without the `brand_packs` table.
    let other = Dir::new();
    let bare = ordin_journal::Datastore::open(ordin_journal::DatastoreConfig::Sqlite {
        path: other.0.join("journal.db"),
        backup_dir: other.0.join("backups"),
    })
    .await
    .unwrap();
    let error = start_public(bare, &other).await.err().expect("no table");
    assert_store_unavailable(&error);
}

fn assert_store_unavailable(error: &BrandError) {
    assert!(matches!(error, BrandError::StoreUnavailable(_)));
    assert_eq!(error.code(), "BRAND_STORE_UNAVAILABLE");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
}

/// `Brand::start` with the real Organisation Profile (defaults) and Localisation.
async fn start_public(
    datastore: ordin_journal::Datastore,
    dir: &Dir,
) -> Result<std::sync::Arc<Brand>, BrandError> {
    struct Clock;
    impl ordin_organisation_profile::Clock for Clock {
        fn now(&self) -> Timestamp {
            Timestamp::now()
        }
    }
    let profile = std::sync::Arc::new(
        ordin_organisation_profile::OrganisationProfileService::start(
            ordin_organisation_profile::ProfileMode::NotUsed,
            ordin_organisation_profile::ProfileOptions {
                refresh_interval: std::time::Duration::from_secs(3600),
                clock: std::sync::Arc::new(Clock),
            },
        )
        .await
        .expect("profile defaults"),
    );
    let localisation = ordin_localisation::Localisation::start(
        ordin_localisation::LocalisationConfig {
            store: ordin_localisation::PreferenceStore::None,
            os_locales: vec!["en-GB".to_owned()],
            app_version: "1.0.0".to_owned(),
        },
        profile.clone(),
    )
    .await
    .expect("localisation");
    Brand::start(
        BrandConfig {
            datastore,
            data_root: dir.0.join("data"),
            http: reqwest::Client::new(),
        },
        profile,
        localisation,
    )
    .await
}

#[tokio::test]
async fn a_stored_row_refused_without_reason_or_the_reverse() {
    let h = Harness::new().await;
    let mut refused_without_reason = received_row("acme-assist", "2.1.0");
    refused_without_reason.state = BrandPackState::Refused;
    assert!(crate::store::insert(h.pool(), &refused_without_reason)
        .await
        .is_err());

    let mut checked_with_reason = received_row("acme-assist", "2.1.1");
    checked_with_reason.state = BrandPackState::ContrastChecked;
    checked_with_reason.refusal_reason = Some("A reason.".to_owned());
    assert!(crate::store::insert(h.pool(), &checked_with_reason)
        .await
        .is_err());

    let mut too_long = received_row("acme-assist", "2.1.2");
    too_long.state = BrandPackState::Refused;
    too_long.refusal_reason = Some("r".repeat(501));
    assert!(crate::store::insert(h.pool(), &too_long).await.is_err());
    assert!(ALL_CODES.contains(&"BRAND_REFUSAL_REASON_MISMATCH"));
}

#[test]
fn every_code_is_registered_and_the_prefix_is_owned() {
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is registered"));
        assert_eq!(entry.owner_crate, "ordin-brand", "{code}");
        let prefix = code.split('_').next().unwrap();
        assert!(owns_prefix("ordin-brand", prefix), "{code}");
    }
    let refusals = [
        RefusalKind::IdInvalid,
        RefusalKind::VersionInvalid,
        RefusalKind::StringsMissing,
        RefusalKind::LocaleDuplicate,
        RefusalKind::StringInvalid,
        RefusalKind::TokensInvalid,
        RefusalKind::FixedToken,
        RefusalKind::ContrastFailed,
        RefusalKind::UrlInvalid,
        RefusalKind::AssetHashMismatch,
        RefusalKind::AssetInvalid,
    ];
    for kind in refusals {
        assert!(ALL_CODES.contains(&kind.code()), "{kind}");
        let error = BrandError::Refused(kind);
        assert_eq!(error.code(), kind.code());
        assert_eq!(error.to_string(), format!("pack refused: {}", kind.code()));
    }
    for (error, code, retryable, repair) in [
        (
            BrandError::AssetHashMismatch,
            "BRAND_ASSET_HASH_MISMATCH",
            false,
            None,
        ),
        (BrandError::AssetInvalid, "BRAND_ASSET_INVALID", false, None),
        (
            BrandError::AssetFetchFailed,
            "BRAND_ASSET_FETCH_FAILED",
            true,
            Some(RepairAction::TryAgain),
        ),
    ] {
        assert_eq!(error.code(), code);
        assert_eq!(error.retryable(), retryable);
        assert_eq!(error.repair(), repair);
    }
}

// spec: ordin-desktop/components/Brand#BRAND_CONTRAST_FAILED
/// Amendment January 2026 (ADR-116), item 2: an optional role is checked on its own value.
#[tokio::test]
async fn an_optional_role_below_its_contrast_threshold() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let mut section = h.section("acme-assist", "2.3.0");
    section
        .design_tokens
        .insert("color.accent-ink.light".to_owned(), "#9999FF".to_owned());
    let row = refused(&h, &section, "BRAND_CONTRAST_FAILED").await;
    // On background (#FFFFFF) it reaches 2.5:1; on surface (#F5F5F5) it fails too.
    assert_eq!(
        reason(&row),
        "Accent text colour on background in the light theme has contrast 2.5:1; at least 4.5:1 is needed, and 1 more."
    );
}
