//! Shared test helpers: a temporary journal database with Brand's table, a `wiremock` asset host on
//! loopback, the brand section of the spec's example, a process-wide log recorder and a stand-in for
//! the desktop program's `brand.get`.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once, PoisonError};
use std::time::Duration;

use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS};
use ordin_localisation::{ActiveLocale, TextDirection};
use ordin_organisation_profile::{
    BrandAsset, BrandLinks, BrandSection, BrandStringsEntry, ProfileSnapshot, SnapshotSource,
    ThemedAsset,
};
use ordin_types::domain::identity::{BrandPack, LocaleSource};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::OrganisationProfileId;
use tokio::sync::watch;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::check::sha256_hex;
use crate::fetch::Timing;
use crate::service::Brand;

// ---- log recorder (plan 01, FU-017: installed globally, before any call site is reached)

#[derive(Debug, Clone)]
pub struct Line {
    pub level: tracing::Level,
    pub event: String,
    pub fields: BTreeMap<String, String>,
}

impl Line {
    pub fn field(&self, name: &str) -> &str {
        self.fields.get(name).map_or("", String::as_str)
    }
}

static LINES: Mutex<Vec<Line>> = Mutex::new(Vec::new());
static ONCE: Once = Once::new();

struct Collect;

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }
}

impl<S: Subscriber> Layer<S> for Collect {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let name = fields.0.remove("message").unwrap_or_default();
        LINES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Line {
                level: *event.metadata().level(),
                event: name,
                fields: fields.0,
            });
    }
}

/// Installs the recorder once, before the test reaches any `tracing` call site.
pub fn install() {
    ONCE.call_once(|| {
        let _ = tracing::subscriber::set_global_default(Registry::default().with(Collect));
    });
}

/// Every line named `event` whose `brand_identifier` is `brand` (tests run in parallel, so each
/// uses its own identifier).
pub fn logged(event: &str, brand: &str) -> Vec<Line> {
    LINES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|line| line.event == event && line.field("brand_identifier") == brand)
        .cloned()
        .collect()
}

pub fn all_lines() -> Vec<Line> {
    LINES.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

// ---- folders and the datastore

/// A folder removed when dropped.
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new() -> Dir {
        let dir = std::env::temp_dir().join(format!("ordin-brand-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Opens the journal database in `dir` and applies the journal's and Brand's migrations.
pub async fn open_store(dir: &Dir) -> Datastore {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("journal.db"),
        backup_dir: dir.0.join("backups"),
    })
    .await
    .expect("open SQLite");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal registers");
    registry
        .register(crate::migrations())
        .expect("brand registers");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply migrations");
    store
}

// ---- the asset host and the brand section

/// The asset files of the example pack, served by the stand-in.
pub struct Assets {
    pub files: HashMap<&'static str, (Vec<u8>, &'static str)>,
}

pub const LOGO_LIGHT: &str = "logo-light.svg";
pub const LOGO_DARK: &str = "logo-dark.svg";
pub const ICONS_LIGHT: &str = "icons-light.svg";
pub const ICONS_DARK: &str = "icons-dark.svg";
pub const FONT_1: &str = "font-1.woff2";
pub const FONT_2: &str = "font-2.woff2";

impl Assets {
    pub fn example() -> Assets {
        let svg = |fill: &str| {
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\">\
                 <rect width=\"10\" height=\"10\" fill=\"{fill}\"/></svg>"
            )
            .into_bytes()
        };
        let mut files = HashMap::new();
        files.insert(LOGO_LIGHT, (svg("#0B5FFF"), "image/svg+xml"));
        files.insert(LOGO_DARK, (svg("#6EA8FF"), "image/svg+xml"));
        files.insert(ICONS_LIGHT, (svg("#1A1A1A"), "image/svg+xml"));
        files.insert(ICONS_DARK, (svg("#F2F2F2"), "image/svg+xml"));
        files.insert(FONT_1, (b"wOF2-acme-sans-regular".to_vec(), "font/woff2"));
        files.insert(FONT_2, (b"wOF2-acme-sans-bold".to_vec(), "font/woff2"));
        Assets { files }
    }

    pub fn asset(&self, server: &MockServer, name: &str) -> BrandAsset {
        let (body, media) = &self.files[name];
        BrandAsset {
            url: format!("{}/brand/{name}", server.uri()),
            sha256: Sha256Hex(sha256_hex(body)),
            media_type: (*media).to_owned(),
        }
    }

    /// Serves every file with 200 at `/brand/<name>`.
    pub async fn serve(&self, server: &MockServer) {
        for (name, (body, media)) in &self.files {
            Mock::given(method("GET"))
                .and(path(format!("/brand/{name}")))
                .respond_with(ResponseTemplate::new(200).set_body_raw(body.clone(), media))
                .mount(server)
                .await;
        }
    }
}

/// The design tokens of a pack whose every contrast pair passes (the build pack's colours).
pub fn passing_tokens() -> BTreeMap<String, String> {
    [
        ("color.text.light", "#1A1A1A"),
        ("color.text-muted.light", "#5C5C5C"),
        ("color.background.light", "#FFFFFF"),
        ("color.surface.light", "#F5F5F5"),
        ("color.accent.light", "#0B5FFF"),
        ("color.on-accent.light", "#FFFFFF"),
        ("color.border-control.light", "#767676"),
        ("color.focus.light", "#0B5FFF"),
        ("color.text.dark", "#F2F2F2"),
        ("color.text-muted.dark", "#B3B3B3"),
        ("color.background.dark", "#121212"),
        ("color.surface.dark", "#1E1E1E"),
        ("color.accent.dark", "#6EA8FF"),
        ("color.on-accent.dark", "#0A0A0A"),
        ("color.border-control.dark", "#8A8A8A"),
        ("color.focus.dark", "#6EA8FF"),
        ("font.body", "Acme Sans"),
        ("space.md", "16px"),
        ("radius.md", "4px"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

pub fn entry(product: &str, coordinator: &str, about: &str) -> BrandStringsEntry {
    BrandStringsEntry {
        product_name: product.to_owned(),
        coordinator_name: coordinator.to_owned(),
        about_text: about.to_owned(),
    }
}

/// The spec's example section ("acme-assist" 2.1.0), with its identifier replaced by `brand` and
/// its assets served by `server`.
pub fn section(server: &MockServer, assets: &Assets, brand: &str, version: &str) -> BrandSection {
    BrandSection {
        brand_identifier: brand.to_owned(),
        version: version.to_owned(),
        strings: BTreeMap::from([(
            "en-GB".to_owned(),
            entry("Acme Assist", "Ada", "Acme Assist helps…"),
        )]),
        logos: ThemedAsset {
            light: assets.asset(server, LOGO_LIGHT),
            dark: assets.asset(server, LOGO_DARK),
        },
        icon_set: ThemedAsset {
            light: assets.asset(server, ICONS_LIGHT),
            dark: assets.asset(server, ICONS_DARK),
        },
        design_tokens: passing_tokens(),
        fonts: vec![assets.asset(server, FONT_1), assets.asset(server, FONT_2)],
        links: BrandLinks {
            help_url: "https://help.acme.example".to_owned(),
            support_url: "https://support.acme.example".to_owned(),
            privacy_url: "https://acme.example/privacy".to_owned(),
        },
        legal_text: BTreeMap::from([("en-GB".to_owned(), "© 2026 Acme Ltd.".to_owned())]),
    }
}

/// A verified profile snapshot carrying `brand`.
pub fn snapshot(brand: Option<BrandSection>, profile_version: &str) -> Arc<ProfileSnapshot> {
    let mut snapshot = ProfileSnapshot::defaults();
    snapshot.document.brand = brand;
    snapshot.source = SnapshotSource::Verified {
        profile_id: OrganisationProfileId::new(),
        profile_version: profile_version.to_owned(),
        activated_at: Timestamp::now(),
    };
    Arc::new(snapshot)
}

pub fn locale(tag: &str, chain: &[&str]) -> ActiveLocale {
    ActiveLocale {
        locale: tag.to_owned(),
        source: LocaleSource::Person,
        direction: TextDirection::Ltr,
        fallback_chain: chain.iter().map(|t| (*t).to_owned()).collect(),
    }
}

// ---- the running service

pub fn timing(recheck_after: Duration) -> Timing {
    Timing {
        fetch_timeout: Duration::from_secs(5),
        backoff: [Duration::from_millis(10), Duration::from_millis(10)],
        jitter_max: Duration::ZERO,
        retry_after_max: Duration::from_secs(60),
        recheck_after,
    }
}

pub struct Harness {
    pub dir: Dir,
    pub store: Datastore,
    pub brand: Arc<Brand>,
    pub profile: watch::Sender<Arc<ProfileSnapshot>>,
    pub locales: watch::Sender<ActiveLocale>,
    pub server: MockServer,
    pub assets: Assets,
}

impl Harness {
    /// A fresh database, the example assets served, and Brand started over a profile without a
    /// brand section and the locale "en-GB".
    pub async fn new() -> Harness {
        Harness::with_recheck(Duration::from_secs(3600)).await
    }

    pub async fn with_recheck(recheck_after: Duration) -> Harness {
        install();
        let dir = Dir::new();
        let store = open_store(&dir).await;
        let server = MockServer::start().await;
        let assets = Assets::example();
        let (profile, locales, brand) =
            start_over(&dir, &store, snapshot(None, "1.0.0"), recheck_after).await;
        Harness {
            dir,
            store,
            brand,
            profile,
            locales,
            server,
            assets,
        }
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    pub fn data_root(&self) -> PathBuf {
        self.dir.0.join("data")
    }

    pub fn section(&self, brand: &str, version: &str) -> BrandSection {
        section(&self.server, &self.assets, brand, version)
    }

    /// Brand started again over the same database and folder, as after a restart.
    pub async fn restart(&mut self) {
        let (profile, locales, brand) = start_over(
            &self.dir,
            &self.store,
            snapshot(None, "1.0.0"),
            Duration::from_secs(3600),
        )
        .await;
        self.profile = profile;
        self.locales = locales;
        self.brand = brand;
    }

    /// The stored row of `(brand, version)` received from a profile.
    pub async fn row(&self, brand: &str, version: &str) -> Option<BrandPack> {
        crate::store::by_key(
            self.pool(),
            brand,
            version,
            ordin_types::domain::identity::BrandPackSource::OrganisationProfile,
        )
        .await
        .expect("read the row")
    }

    pub async fn rows(&self) -> Vec<BrandPack> {
        crate::store::all_newest_first(self.pool())
            .await
            .expect("read the rows")
    }
}

/// Brand over `store`, with fresh profile and locale channels. The profile receiver is not marked
/// changed: the follower acts only on snapshots a test sends, so a test that calls `receive`
/// directly is not raced by the initial snapshot.
pub async fn start_over(
    dir: &Dir,
    store: &Datastore,
    initial: Arc<ProfileSnapshot>,
    recheck_after: Duration,
) -> (
    watch::Sender<Arc<ProfileSnapshot>>,
    watch::Sender<ActiveLocale>,
    Arc<Brand>,
) {
    let (profile, profile_rx) = watch::channel(initial);
    let first = locale("en-GB", &["en-GB"]);
    let (locales, locale_rx) = watch::channel(first.clone());
    let brand = Brand::start_with_sources(
        store.sqlite().expect("SQLite").clone(),
        dir.0.join("data"),
        reqwest::Client::new(),
        timing(recheck_after),
        profile_rx,
        first,
        locale_rx,
    )
    .await
    .expect("start Brand");
    (profile, locales, brand)
}

/// Waits (up to 10 s) until `check` holds; the followers run on their own tasks.
pub async fn eventually<F: FnMut() -> bool>(mut check: F) {
    for _ in 0..1000 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the condition did not hold within 10 s");
}

/// The `brand.get` reply as the desktop program composes it (Service API table).
pub fn brand_get(brand: &Brand) -> serde_json::Value {
    let pack = brand.active();
    let strings = brand.strings();
    let refusal = brand.last_refusal().map(|refused| {
        serde_json::json!({
            "brand_identifier": refused.brand_identifier,
            "version": refused.version,
            "reason": refused.refusal_reason,
            "received_at": refused.received_at,
        })
    });
    serde_json::json!({
        "brand_identifier": pack.brand_identifier,
        "version": pack.version,
        "source": pack.source,
        "strings": strings,
        "design_tokens": pack.design_tokens,
        "links": {
            "help_url": pack.help_url,
            "support_url": pack.support_url,
            "privacy_url": pack.privacy_url,
        },
        "font_count": pack.font_file_paths.len(),
        "refusal": refusal,
    })
}

/// A unique brand identifier per test (the log recorder is shared by every test).
pub fn unique(prefix: &str) -> String {
    let id = uuid::Uuid::now_v7().simple().to_string();
    format!("{prefix}-{}", &id[id.len() - 10..])
}
