//! Observability (Brand.md; Logging.md R-2): the log lines Brand writes through its public
//! interface, their fields, and what they never hold. The only test in its own binary: `tracing`
//! caches per call site whether anything listens, so one process-wide subscriber is installed before
//! anything runs and no other test shares the process (plan 01, FU-017).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_brand::{Brand, BrandConfig};
use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS};
use ordin_localisation::{Localisation, LocalisationConfig, PreferenceStore};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_types::time::Timestamp;
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn lines(buffer: &Buffer) -> Vec<serde_json::Value> {
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    text.lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect()
}

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// A folder removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn start(store: &Datastore, data_root: PathBuf) -> Arc<Brand> {
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::NotUsed,
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("profile defaults"),
    );
    let localisation = Localisation::start(
        LocalisationConfig {
            store: PreferenceStore::None,
            os_locales: vec!["en-GB".to_owned()],
            app_version: "1.0.0".to_owned(),
        },
        profile.clone(),
    )
    .await
    .expect("localisation");
    Brand::start(
        BrandConfig {
            datastore: store.clone(),
            data_root,
            http: reqwest::Client::new(),
        },
        profile,
        localisation,
    )
    .await
    .unwrap_or_else(|e| panic!("start Brand: {e}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn each_event_is_logged_with_its_fields_and_never_with_brand_text_or_paths() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    tracing::subscriber::set_global_default(subscriber).expect("install the subscriber");

    let dir = Dir(std::env::temp_dir().join(format!("ordin-brand-log-{}", uuid::Uuid::now_v7())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("journal.db"),
        backup_dir: dir.0.join("backups"),
    })
    .await
    .unwrap();
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(ordin_brand::migrations()).unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let data_root = dir.0.join("data");

    // First start: the build pack is installed and active.
    let brand = start(&store, data_root.clone()).await;
    assert_eq!(brand.active().brand_identifier, "ordin");
    drop(brand);

    // A profile pack left active whose asset files are gone.
    let pool = store.sqlite().unwrap();
    sqlx::query("UPDATE brand_packs SET state = 'contrast_checked' WHERE state = 'active'")
        .execute(pool)
        .await
        .unwrap();
    let missing = data_root
        .join("brand/acme-log/2.1.0/missing.svg")
        .to_string_lossy()
        .into_owned();
    let strings = r#"[{"locale":"en-GB","product_name":"Acme Assist","coordinator_name":"Ada","about_text":"Acme Assist helps.","legal_text":"(c) Acme Ltd."}]"#;
    sqlx::query(
        "INSERT INTO brand_packs (id, brand_identifier, version, source, strings, logo_light_path, \
         logo_dark_path, icon_set_light_path, icon_set_dark_path, design_tokens, font_file_paths, \
         help_url, support_url, privacy_url, state, refusal_reason, received_at, activated_at) \
         VALUES (?, 'acme-log', '2.1.0', 'organisation_profile', ?, ?, ?, ?, ?, '{}', '[]', \
         'https://help.acme.example', 'https://support.acme.example', \
         'https://acme.example/privacy', 'active', NULL, ?, ?)",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(strings)
    .bind(&missing)
    .bind(&missing)
    .bind(&missing)
    .bind(&missing)
    .bind(Timestamp::now().to_wire())
    .bind(Timestamp::now().to_wire())
    .execute(pool)
    .await
    .unwrap();

    // Second start: the damaged pack gives way to the build pack.
    let brand = start(&store, data_root.clone()).await;
    assert_eq!(brand.active().brand_identifier, "ordin");

    let all = lines(&buffer);
    let named = |event: &str| -> Vec<&serde_json::Value> {
        all.iter().filter(|line| line["event"] == event).collect()
    };
    let activated = named("brand.activated");
    assert!(!activated.is_empty(), "brand.activated was written");
    let last = activated.last().unwrap();
    assert_eq!(last["brand_identifier"], "ordin");
    assert_eq!(last["version"], "1.3.0");
    assert_eq!(last["source"], "build");
    let missing_lines = named("brand.asset_missing");
    assert_eq!(missing_lines.len(), 1);
    assert_eq!(missing_lines[0]["brand_identifier"], "acme-log");
    assert_eq!(missing_lines[0]["version"], "2.1.0");
    assert_eq!(missing_lines[0]["asset"], "logo_light");

    let root = data_root.to_string_lossy().into_owned();
    let mut seen = 0;
    for line in &all {
        if line["logger"]
            .as_str()
            .is_some_and(|l| l.starts_with("ordin_brand"))
        {
            seen += 1;
            assert_eq!(line["component"], "brand", "{line}");
            assert!(line.get("dropped_fields").is_none(), "{line}");
            assert_ne!(line["event"], "invalid_event_name", "{line}");
            let text = line.to_string();
            assert!(
                !text.contains("<debug>"),
                "ids are logged as strings: {text}"
            );
            assert!(!text.contains("Acme Assist"), "no brand text: {text}");
            assert!(!text.contains("acme.example"), "no link: {text}");
            assert!(!text.contains(&root), "no path: {text}");
        }
    }
    assert!(seen >= 2, "Brand's own lines were checked");
}
