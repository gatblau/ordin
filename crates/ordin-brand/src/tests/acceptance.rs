//! X2: one test per Gherkin scenario of Brand.md *Acceptance Criteria*, with the same name.

use std::sync::Arc;
use std::time::Duration;

use ordin_journal::{Journal, JournalOptions, SystemClock};
use ordin_types::domain::identity::{BrandPackSource, BrandPackState};
use ordin_types::wire::events::EventKind;
use ordin_types::CodedError as _;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::support::*;
use crate::{AssetRef, BrandError};

/// The desktop program's forwarder: each change of `subscribe()` is appended as `brand_changed`.
async fn forward_brand_changed(h: &Harness) -> Arc<Journal> {
    let journal = Arc::new(
        Journal::start(
            h.store.clone(),
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("start the journal"),
    );
    let mut changes = h.brand.subscribe();
    let sink = Arc::clone(&journal);
    tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            let pack = changes.borrow_and_update().clone();
            let data = json!({
                "brand_identifier": pack.brand_identifier,
                "version": pack.version,
                "source": pack.source,
            });
            sink.append_event(EventKind::BrandChanged.as_str(), data)
                .await
                .expect("append brand_changed");
        }
    });
    journal
}

async fn brand_changed_events(journal: &Journal) -> Vec<serde_json::Value> {
    if journal.last_seq() == 0 {
        return Vec::new();
    }
    journal
        .events_since(0, 1_000)
        .await
        .expect("read events")
        .into_iter()
        .filter(|event| event.kind == "brand_changed")
        .map(|event| event.data)
        .collect()
}

async fn wait_for_events(journal: &Journal, at_least: usize) -> Vec<serde_json::Value> {
    for _ in 0..500 {
        let events = brand_changed_events(journal).await;
        if events.len() >= at_least {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("fewer than {at_least} brand_changed events");
}

fn with_french(
    h: &Harness,
    brand: &str,
    version: &str,
) -> ordin_organisation_profile::BrandSection {
    let mut section = h.section(brand, version);
    section.strings.insert(
        "fr-FR".to_owned(),
        entry("Acme Assistant", "Adèle", "Acme Assistant aide…"),
    );
    section
        .legal_text
        .insert("fr-FR".to_owned(), "© 2026 Acme SARL.".to_owned());
    section
}

#[tokio::test]
async fn happy_path_profile_brand_becomes_active() {
    let h = Harness::new().await;
    let start = h.brand.active();
    assert_eq!(
        (start.brand_identifier.as_str(), start.version.as_str()),
        ("ordin", "1.3.0")
    );
    assert_eq!(start.source, BrandPackSource::Build);
    h.assets.serve(&h.server).await;
    let journal = forward_brand_changed(&h).await;

    h.profile
        .send(snapshot(Some(h.section("acme-assist", "2.1.0")), "1.1.0"))
        .expect("send the snapshot");
    eventually(|| h.brand.active().brand_identifier == "acme-assist").await;

    let row = h.row("acme-assist", "2.1.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Active);
    assert!(row.activated_at.is_some());
    assert!(!logged("brand.received", "acme-assist").is_empty());
    assert!(!logged("brand.activated", "acme-assist").is_empty());
    let build = h
        .rows()
        .await
        .into_iter()
        .find(|r| r.source == BrandPackSource::Build)
        .expect("the build row");
    assert_eq!(build.state, BrandPackState::ContrastChecked);

    let reply = brand_get(&h.brand);
    assert_eq!(reply["brand_identifier"], "acme-assist");
    assert_eq!(reply["version"], "2.1.0");
    assert_eq!(reply["source"], "organisation_profile");
    assert_eq!(reply["strings"]["product_name"], "Acme Assist");
    assert_eq!(reply["strings"]["coordinator_name"], "Ada");
    assert_eq!(reply["strings"]["legal_text"], "© 2026 Acme Ltd.");
    assert_eq!(reply["font_count"], 2);
    assert_eq!(reply["refusal"], serde_json::Value::Null);
    assert_eq!(h.brand.coordinator_name(), "Ada");

    let events = wait_for_events(&journal, 1).await;
    assert_eq!(
        events.last().expect("an event"),
        &json!({"brand_identifier": "acme-assist", "version": "2.1.0", "source": "organisation_profile"})
    );
}

#[tokio::test]
async fn error_contrast_failure_keeps_the_build_brand() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let mut section = h.section("acme-assist", "2.2.0");
    section
        .design_tokens
        .insert("color.text.dark".to_owned(), "#636363".to_owned());

    let error = h.brand.receive(&section).await.expect_err("refused");
    assert_eq!(error.code(), "BRAND_CONTRAST_FAILED");
    let row = h.row("acme-assist", "2.2.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Refused);
    let reason = row.refusal_reason.expect("a reason");
    assert!(
        reason.contains("Text colour on background in the dark theme"),
        "{reason}"
    );
    assert!(reason.contains("3.1:1"), "{reason}");
    assert_eq!(h.brand.active().source, BrandPackSource::Build);

    let reply = brand_get(&h.brand);
    assert_eq!(reply["brand_identifier"], "ordin");
    assert_eq!(reply["refusal"]["brand_identifier"], "acme-assist");
    assert_eq!(reply["refusal"]["version"], "2.2.0");
    assert_eq!(reply["refusal"]["reason"], reason.as_str());
    assert!(reply["refusal"]["received_at"].is_string());
}

#[tokio::test]
async fn error_fixed_safety_colour() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let mut section = h.section("acme-assist", "2.2.0");
    section
        .design_tokens
        .insert("color.danger.light".to_owned(), "#00FF00".to_owned());

    let error = h.brand.receive(&section).await.expect_err("refused");
    assert_eq!(error.code(), "BRAND_FIXED_TOKEN");
    let row = h.row("acme-assist", "2.2.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Refused);
    assert_eq!(
        row.refusal_reason.as_deref(),
        Some("The brand tries to change color.danger.light, which ordin keeps fixed.")
    );
}

#[tokio::test]
async fn error_asset_checksum_mismatch() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let mut section = h.section("acme-assist", "2.2.0");
    section.logos.light.sha256 =
        ordin_types::num::Sha256Hex(crate::check::sha256_hex(b"another logo"));

    let error = h.brand.receive(&section).await.expect_err("refused");
    assert_eq!(error.code(), "BRAND_ASSET_HASH_MISMATCH");
    let row = h.row("acme-assist", "2.2.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Refused);
    assert!(!h
        .data_root()
        .join("brand")
        .join("acme-assist")
        .join("2.2.0")
        .exists());
}

#[tokio::test]
async fn error_svg_with_script() {
    let mut h = Harness::new().await;
    h.assets.files.insert(
        LOGO_DARK,
        (
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>".to_vec(),
            "image/svg+xml",
        ),
    );
    h.assets.serve(&h.server).await;
    let section = h.section("acme-assist", "2.2.0");

    let error = h.brand.receive(&section).await.expect_err("refused");
    assert_eq!(error.code(), "BRAND_ASSET_INVALID");
    let row = h.row("acme-assist", "2.2.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Refused);
    assert_eq!(
        row.refusal_reason.as_deref(),
        Some("The dark logo file isn't an accepted image.")
    );
}

#[tokio::test]
async fn edge_download_fails_and_is_retried() {
    let h = Harness::with_recheck(Duration::from_millis(800)).await;
    let brand = unique("acme");
    Mock::given(method("GET"))
        .and(path(format!("/brand/{LOGO_LIGHT}")))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(3)
        .with_priority(1)
        .mount(&h.server)
        .await;
    h.assets.serve(&h.server).await;

    h.profile
        .send(snapshot(Some(h.section(&brand, "2.1.0")), "1.1.0"))
        .expect("send the snapshot");
    eventually(|| !logged("brand.asset_fetch_failed", &brand).is_empty()).await;
    let row = h.row(&brand, "2.1.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Received);
    assert_eq!(h.brand.active().source, BrandPackSource::Build);
    let line = &logged("brand.asset_fetch_failed", &brand)[0];
    assert_eq!(line.level, tracing::Level::WARN);
    assert_eq!(line.field("asset"), "logo_light");
    assert_eq!(line.field("http_status"), "503");
    assert_eq!(line.field("attempt"), "3");
    assert_eq!(line.field("version"), "2.1.0");

    // No new snapshot arrives: the timer runs the check again (15 minutes in production).
    eventually(|| h.brand.active().brand_identifier == brand).await;
    let row = h.row(&brand, "2.1.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Active);
}

#[tokio::test]
async fn edge_same_refused_version_is_not_retried() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let mut section = h.section("acme-assist", "2.2.0");
    section
        .design_tokens
        .insert("color.waiting.dark".to_owned(), "#FFAA00".to_owned());
    h.brand.receive(&section).await.expect_err("refused");
    let before = h.rows().await.len();
    let requests = h.server.received_requests().await.unwrap_or_default().len();

    h.brand.receive(&section).await.expect("nothing to do");
    assert_eq!(h.rows().await.len(), before, "no new row is created");
    assert_eq!(
        h.server.received_requests().await.unwrap_or_default().len(),
        requests,
        "nothing is fetched again"
    );
    let row = h.row("acme-assist", "2.2.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Refused);
}

#[tokio::test]
async fn edge_profile_removes_its_brand_section() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    h.brand
        .receive(&h.section("acme-assist", "2.1.0"))
        .await
        .expect("activated");
    assert_eq!(h.brand.active().brand_identifier, "acme-assist");

    h.profile
        .send(snapshot(None, "1.2.0"))
        .expect("send the snapshot");
    eventually(|| h.brand.active().source == BrandPackSource::Build).await;
    assert_eq!(h.brand.active().brand_identifier, "ordin");
    let row = h.row("acme-assist", "2.1.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::ContrastChecked);
}

#[tokio::test]
async fn edge_strings_follow_the_locale() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    h.brand
        .receive(&with_french(&h, "acme-assist", "2.1.0"))
        .await
        .expect("activated");
    assert_eq!(
        brand_get(&h.brand)["strings"]["product_name"],
        "Acme Assist"
    );

    h.locales
        .send(locale("fr-FR", &["fr-FR", "en-GB"]))
        .expect("send the locale");
    eventually(|| h.brand.strings().locale == "fr-FR").await;
    assert_eq!(
        brand_get(&h.brand)["strings"]["product_name"],
        "Acme Assistant"
    );

    h.locales
        .send(locale("de-DE", &["de-DE", "fr-FR", "en-GB"]))
        .expect("send the locale");
    eventually(|| h.brand.strings().coordinator_name == "Adèle").await;
    let reply = brand_get(&h.brand);
    assert_eq!(reply["strings"]["locale"], "fr-FR");
    assert_eq!(reply["strings"]["product_name"], "Acme Assistant");

    // No locale of the chain has strings: the pack's first entry.
    h.locales
        .send(locale("ja-JP", &["ja-JP"]))
        .expect("send the locale");
    eventually(|| h.brand.strings().locale == "en-GB").await;
}

#[tokio::test]
async fn edge_damaged_asset_on_disk() {
    let mut h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let brand = unique("acme");
    h.brand
        .receive(&h.section(&brand, "2.1.0"))
        .await
        .expect("activated");
    std::fs::remove_file(&h.brand.active().logo_dark_path).expect("delete the dark logo");

    h.restart().await;
    assert_eq!(h.brand.active().source, BrandPackSource::Build);
    let lines = logged("brand.asset_missing", &brand);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].level, tracing::Level::WARN);
    assert_eq!(lines[0].field("asset"), "logo_dark");
    assert_eq!(lines[0].field("version"), "2.1.0");
}

#[tokio::test]
async fn error_asset_index_unknown() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    h.brand
        .receive(&h.section("acme-assist", "2.1.0"))
        .await
        .expect("activated");
    assert_eq!(h.brand.active().font_file_paths.len(), 2);

    let which: AssetRef = serde_json::from_value(json!({"kind": "font", "index": 9})).unwrap();
    let error = h.brand.asset(which).await.expect_err("unknown");
    assert!(matches!(error, BrandError::AssetUnknown));
    assert_eq!(error.code(), "BRAND_ASSET_UNKNOWN");
}

#[tokio::test]
async fn error_a_refused_pack_carries_its_repair_action() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    h.brand
        .receive(&h.section("acme-assist", "2.1.0"))
        .await
        .expect("activated");
    let mut failing = h.section("acme-assist", "2.2.0");
    failing
        .design_tokens
        .insert("color.on-accent.light".to_owned(), "#3B6FFF".to_owned());

    let error = h.brand.receive(&failing).await.expect_err("refused");
    assert_eq!(error.code(), "BRAND_CONTRAST_FAILED");
    assert_eq!(error.repair(), None);
    assert!(!error.retryable());
    let active = h.brand.active();
    assert_eq!(
        (active.brand_identifier.as_str(), active.version.as_str()),
        ("acme-assist", "2.1.0")
    );
}

#[tokio::test]
async fn happy_path_a_locale_change_re_announces_the_brand() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    h.brand
        .receive(&with_french(&h, "acme-assist", "2.1.0"))
        .await
        .expect("activated");
    let journal = forward_brand_changed(&h).await;
    let before = brand_changed_events(&journal).await.len();

    h.locales
        .send(locale("fr-FR", &["fr-FR", "en-GB"]))
        .expect("send the locale");
    eventually(|| h.brand.strings().locale == "fr-FR").await;
    assert_eq!(h.brand.strings().product_name, "Acme Assistant");
    let events = wait_for_events(&journal, before + 1).await;
    assert_eq!(
        events.last().expect("an event"),
        &json!({"brand_identifier": "acme-assist", "version": "2.1.0", "source": "organisation_profile"})
    );
}

/// Amendment January 2026 (ADR-116), item 3: the build pack carries the prototype's 37 tokens and
/// passes every pair of C5 and of item 2.
#[tokio::test]
async fn happy_path_the_build_pack_passes_every_pair() {
    let h = Harness::new().await;
    let active = h.brand.active();
    assert_eq!(active.source, BrandPackSource::Build);
    assert_eq!(active.version, "1.3.0");
    assert_eq!(active.design_tokens.len(), 37);
    assert_eq!(active.design_tokens["color.background.light"], "#F2F4F7");
    assert_eq!(active.design_tokens["color.border-control.dark"], "#647384");
    assert_eq!(active.design_tokens["font.body"], "Jost");
    crate::check::check_tokens(&active.design_tokens).expect("C4 passes");
    crate::check::check_contrast(&active.design_tokens).expect("every pair passes");
}

/// Amendment January 2026 (ADR-116), item 2: a pack with only the eight required colour roles,
/// valid before the amendment, stays valid.
#[tokio::test]
async fn edge_case_a_pack_with_only_the_required_roles_stays_valid() {
    let h = Harness::new().await;
    h.assets.serve(&h.server).await;
    let section = h.section("acme-assist", "3.0.0");
    assert!(
        !section.design_tokens.keys().any(|name| {
            crate::check::OPTIONAL_COLOUR_ROLES
                .iter()
                .any(|(role, _)| name.starts_with(&format!("color.{role}.")))
        }),
        "the pack carries no optional role"
    );
    h.brand.receive(&section).await.expect("accepted");
    let row = h.row("acme-assist", "3.0.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Active);
}

/// Amendment January 2026 (ADR-116), item 3: a computer that holds build pack 1.0.0 stores the newer build pack
/// at start (A2) and activates it there, in place of the older build pack (A3, B1's rule).
#[tokio::test]
async fn edge_case_a_computer_holding_build_pack_one_zero_zero() {
    let mut h = Harness::new().await;
    let current = h.brand.active();
    assert_eq!(current.version, "1.3.0");
    // Replace the current row by an active 1.0.0 build row with the same files, as an older install left it.
    crate::store::delete(h.pool(), current.id).await.unwrap();
    let mut older = (*current).clone();
    older.id = ordin_types::BrandPackId::new();
    older.version = "1.0.0".to_owned();
    older.state = BrandPackState::Active;
    crate::store::insert(h.pool(), &older).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;

    h.restart().await;

    // A3 restores the active row and gives an older build row way to the newest build row.
    let active = h.brand.active();
    assert_eq!(
        (active.source, active.version.as_str()),
        (BrandPackSource::Build, "1.3.0")
    );
    let rows = h.rows().await;
    let old = rows
        .iter()
        .find(|row| row.version == "1.0.0")
        .expect("1.0.0 kept");
    assert_ne!(old.state, BrandPackState::Active);
}

// ---- Amendment January 2026 — The prototype's icons in the build pack (ADR-117)

/// The 50 icon names of `cross-cutting/VisualDesign.md` V8, in V8's order.
const V8_ICON_NAMES: [&str; 50] = [
    "i-chev",
    "i-ws",
    "i-docs",
    "i-sp",
    "i-cf",
    "i-gl",
    "i-gh",
    "i-lc",
    "i-folder",
    "i-word",
    "i-sheet",
    "i-slides",
    "i-pdf",
    "i-page",
    "i-md",
    "i-image",
    "i-agent",
    "i-remote",
    "i-task",
    "i-crew",
    "i-keptcrew",
    "i-run",
    "i-mail",
    "i-chart",
    "i-memory",
    "i-know",
    "i-mic",
    "i-more",
    "i-plus",
    "i-chat",
    "i-open",
    "i-send",
    "i-publish",
    "i-remove",
    "i-bin",
    "i-stop",
    "i-play",
    "i-edit",
    "i-info",
    "i-key",
    "i-search",
    "i-home",
    "i-eye",
    "i-project",
    "i-code",
    "i-history",
    "i-copy",
    "i-term",
    "i-fold",
    "i-unfold",
];

/// Each `<symbol>` of an SVG sprite as (id, viewBox), in document order.
fn symbols(svg: &str) -> Vec<(String, String)> {
    let attribute = |tag: &str, name: &str| -> String {
        let key = format!(" {name}=\"");
        tag.find(&key)
            .map(|at| {
                let rest = &tag[at + key.len()..];
                rest[..rest.find('"').unwrap_or(rest.len())].to_owned()
            })
            .unwrap_or_default()
    };
    svg.match_indices("<symbol")
        .map(|(at, _)| {
            let tag = &svg[at..at + svg[at..].find('>').expect("a closed tag")];
            (attribute(tag, "id"), attribute(tag, "viewBox"))
        })
        .collect()
}

async fn icon_set_text(h: &Harness, which: AssetRef) -> String {
    use base64::Engine as _;
    let bytes = h.brand.asset(which).await.expect("the icon set");
    assert_eq!(bytes.media_type, "image/svg+xml");
    let raw = base64::engine::general_purpose::STANDARD
        .decode(bytes.base64)
        .expect("base64");
    String::from_utf8(raw).expect("UTF-8")
}

// spec: ordin-desktop/components/Brand#ADR-117
#[tokio::test]
async fn happy_path_the_build_pack_carries_the_50_icons() {
    let h = Harness::new().await;
    let active = h.brand.active();
    assert_eq!(
        (active.source, active.version.as_str()),
        (BrandPackSource::Build, "1.3.0")
    );
    let light = icon_set_text(&h, AssetRef::IconSetLight).await;
    let dark = icon_set_text(&h, AssetRef::IconSetDark).await;
    assert_eq!(light, dark, "the light and dark icon sets are the same SVG");
    assert!(light.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\">"));
    let found = symbols(&light);
    let names: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(names, V8_ICON_NAMES);
    assert!(found.iter().all(|(_, view_box)| view_box == "0 0 16 16"));
    assert!(!light.contains("ordin-mark"));
    assert!(!light.contains(" fill=") && !light.contains(" stroke=") && !light.contains(" style="));
    assert_eq!(
        light.matches("stroke-width=").count(),
        1,
        "only i-more's path"
    );
    // C2 passes on the compiled-in bytes (build.rs ran the same check at build time).
    let sha = crate::check::sha256_hex(light.as_bytes());
    crate::check::check_asset(
        crate::check::AssetSlot::IconSetLight,
        &sha,
        "image/svg+xml",
        light.as_bytes(),
    )
    .expect("C2 passes");
}

/// A computer that holds build pack 1.1.0 stores the current build pack at start (A2) and activates it (B1's rule).
#[tokio::test]
async fn edge_case_a_computer_holding_build_pack_one_one_zero() {
    let mut h = Harness::new().await;
    let current = h.brand.active();
    assert_eq!(current.version, "1.3.0");
    // Replace the current row by an active 1.1.0 build row, as the previous release left it.
    crate::store::delete(h.pool(), current.id).await.unwrap();
    let mut older = (*current).clone();
    older.id = ordin_types::BrandPackId::new();
    older.version = "1.1.0".to_owned();
    older.state = BrandPackState::Active;
    crate::store::insert(h.pool(), &older).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;

    h.restart().await;

    let rows = h.rows().await;
    let new = rows
        .iter()
        .find(|row| row.source == BrandPackSource::Build && row.version == "1.3.0")
        .expect("the current build pack stored");
    assert_eq!(new.state, BrandPackState::Active);
    let old = rows
        .iter()
        .find(|row| row.version == "1.1.0")
        .expect("1.1.0 kept");
    assert_ne!(old.state, BrandPackState::Active);
    assert_eq!(
        (h.brand.active().source, h.brand.active().version.as_str()),
        (BrandPackSource::Build, "1.3.0")
    );
}

#[tokio::test]
async fn edge_case_a_company_pack_without_icon_names_stays_valid() {
    let mut h = Harness::new().await;
    let brand = unique("acme-mark");
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><symbol id=\"acme-mark\" viewBox=\"0 0 24 24\">\
                <circle cx=\"12\" cy=\"12\" r=\"9\"/></symbol></svg>"
        .to_vec();
    h.assets
        .files
        .insert(ICONS_LIGHT, (svg.clone(), "image/svg+xml"));
    h.assets.files.insert(ICONS_DARK, (svg, "image/svg+xml"));
    h.assets.serve(&h.server).await;

    h.brand
        .receive(&h.section(&brand, "4.0.0"))
        .await
        .expect("not refused for its icon set");
    let row = h.row(&brand, "4.0.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Active);
    assert_eq!(row.refusal_reason, None);
}

#[tokio::test]
async fn edge_case_a_company_pack_with_a_png_icon_set_stays_valid() {
    let mut h = Harness::new().await;
    let brand = unique("acme-png");
    let png = b"\x89PNG\r\n\x1a\nicons".to_vec();
    h.assets
        .files
        .insert(ICONS_LIGHT, (png.clone(), "image/png"));
    h.assets.files.insert(ICONS_DARK, (png, "image/png"));
    h.assets.serve(&h.server).await;

    h.brand
        .receive(&h.section(&brand, "4.0.0"))
        .await
        .expect("not refused for its icon set");
    let row = h.row(&brand, "4.0.0").await.expect("the row");
    assert_eq!(row.state, BrandPackState::Active);
    assert_eq!(row.refusal_reason, None);
    let bytes = h.brand.asset(AssetRef::IconSetLight).await.expect("served");
    assert_eq!(bytes.media_type, "image/png");
}
