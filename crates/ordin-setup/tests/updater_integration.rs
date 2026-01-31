//! Integration: the Setup Assistant with the real Organisation Profile service and the Journal's
//! SQLite datastore, its update check against a feed in Tauri's format served by `wiremock`, and a
//! simulated restart on the same datastore after the update installs, where **What's new** shows
//! the new version's notes once.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, ModuleMigrations, MIGRATIONS,
};
use ordin_organisation_profile::{
    Clock, InstallerSettings, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
    MODULE_NAME, SQLITE_MIGRATIONS,
};
use ordin_setup::{
    AppUpdater, InstallKind, SetupAssistant, SetupConfig, SetupProgram, UpdateFinish, UpdateOffer,
    UpdateState, UpdaterError,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PLATFORM: &str = "linux-x86_64";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-setup-it-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(std::fs::canonicalize(&dir).expect("canonical"))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().expect("lock")
}

/// A stand-in for Tauri's updater: reads the feed in Tauri's format over HTTP, downloads the
/// platform's file, and records the install. Signature checking is the real updater's; this one
/// keeps the signature it was given.
struct FeedUpdater {
    http: reqwest::Client,
    running: String,
    found: Mutex<Option<(String, String)>>,
    downloaded: Mutex<Option<Vec<u8>>>,
    installs: AtomicU32,
}

#[async_trait::async_trait]
impl AppUpdater for FeedUpdater {
    async fn check(&self, feed_url: &url::Url) -> Result<Option<UpdateOffer>, UpdaterError> {
        let feed: serde_json::Value = self
            .http
            .get(feed_url.clone())
            .send()
            .await
            .map_err(|_| UpdaterError::Unreachable)?
            .json()
            .await
            .map_err(|_| UpdaterError::Unreachable)?;
        let version = feed["version"].as_str().ok_or(UpdaterError::Unreachable)?;
        let newer = semver::Version::parse(version).map_err(|_| UpdaterError::Unreachable)?
            > semver::Version::parse(&self.running).expect("running version");
        if !newer {
            return Ok(None);
        }
        let platform = &feed["platforms"][PLATFORM];
        let (Some(url), Some(_signature)) =
            (platform["url"].as_str(), platform["signature"].as_str())
        else {
            return Ok(None);
        };
        *lock(&self.found) = Some((version.to_owned(), url.to_owned()));
        Ok(Some(UpdateOffer {
            version: version.to_owned(),
            notes: feed["notes"].as_str().unwrap_or_default().to_owned(),
            state: UpdateState::Downloading,
        }))
    }

    async fn download(&self, offer: &UpdateOffer) -> Result<(), UpdaterError> {
        let url = lock(&self.found)
            .clone()
            .filter(|(v, _)| *v == offer.version)
            .map(|(_, u)| u)
            .ok_or(UpdaterError::NothingDownloaded)?;
        let reply = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| UpdaterError::Unreachable)?;
        if !reply.status().is_success() {
            return Err(UpdaterError::Unreachable);
        }
        let bytes = reply.bytes().await.map_err(|_| UpdaterError::Unreachable)?;
        *lock(&self.downloaded) = Some(bytes.to_vec());
        Ok(())
    }

    async fn install_and_restart(&self) -> Result<(), UpdaterError> {
        if lock(&self.downloaded).is_none() {
            return Err(UpdaterError::NothingDownloaded);
        }
        self.installs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn install_on_exit(&self) -> Result<(), UpdaterError> {
        self.install_and_restart().await
    }

    fn install_kind(&self) -> InstallKind {
        InstallKind::PerUser
    }

    fn other_install(&self) -> Option<PathBuf> {
        None
    }
}

async fn open_store(data_root: &Path) -> Datastore {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data_root.join("journal.db"),
        backup_dir: data_root.join("backups"),
    })
    .await
    .expect("datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry
        .register(ModuleMigrations {
            module: MODULE_NAME,
            sqlite: SQLITE_MIGRATIONS,
            postgres: &[],
        })
        .expect("profile");
    registry.register(ordin_setup::migrations()).expect("setup");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("migrations");
    store
}

/// The program's start: the real profile service, with the installer's feed address as the build
/// default (no profile address, so no organisation profile is fetched).
async fn start(
    data_root: &Path,
    store: &Datastore,
    feed_url: &str,
    version: &str,
) -> (SetupAssistant, Arc<OrganisationProfileService>) {
    let http = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client");
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::Read {
                pool: store.sqlite().expect("sqlite").clone(),
                data_root: data_root.to_path_buf(),
                http: http.clone(),
                build_defaults: InstallerSettings {
                    update_feed_url: Some(feed_url.to_owned()),
                    ..InstallerSettings::default()
                },
            },
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(WallClock),
            },
        )
        .await
        .expect("profile service"),
    );
    let setup = SetupAssistant::with_profile_only(
        profile.clone(),
        SetupConfig {
            program: SetupProgram::Desktop,
            bundled_programs: Vec::new(),
            application_version: version.to_owned(),
            http,
        },
        store.clone(),
    )
    .expect("setup");
    (setup, profile)
}

async fn until_ready(setup: &SetupAssistant) -> UpdateOffer {
    for _ in 0..200 {
        if let Some(offer) = setup
            .update_offer()
            .filter(|o| o.state == UpdateState::Ready)
        {
            return offer;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the update never became ready: {:?}", setup.update_offer());
}

#[tokio::test]
async fn an_update_from_a_tauri_feed_installs_and_whats_new_shows_after_the_restart() {
    let dir = TempDir::new();
    let data_root = dir.0.join("data");
    std::fs::create_dir_all(&data_root).expect("data root");
    let feed = MockServer::start().await;
    let feed_url = format!("{}/ordin/latest.json", feed.uri());
    Mock::given(method("GET"))
        .and(path("/ordin/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "version": "0.2.0",
            "notes": "Faster search and a new Home page.",
            "pub_date": "2026-01-10T08:00:00Z",
            "platforms": {
                PLATFORM: {
                    "url": format!("{}/ordin/ordin_0.2.0_amd64.AppImage.tar.gz", feed.uri()),
                    "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZQ=="
                }
            }
        })))
        .mount(&feed)
        .await;
    Mock::given(method("GET"))
        .and(path("/ordin/ordin_0.2.0_amd64.AppImage.tar.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"the 0.2.0 program".to_vec()))
        .expect(1)
        .mount(&feed)
        .await;

    // Running 0.1.0: the check finds 0.2.0 and downloads it without asking.
    let store = open_store(&data_root).await;
    let (setup, profile) = start(&data_root, &store, &feed_url, "0.1.0").await;
    assert_eq!(
        setup.effective_update_feed().as_deref(),
        Some(feed_url.as_str())
    );
    let updater = Arc::new(FeedUpdater {
        http: reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client"),
        running: "0.1.0".to_owned(),
        found: Mutex::new(None),
        downloaded: Mutex::new(None),
        installs: AtomicU32::new(0),
    });
    setup.register_updater(updater.clone());
    let offer = setup.check_update().await.expect("check").expect("offer");
    assert_eq!(offer.version, "0.2.0");
    let ready = until_ready(&setup).await;
    assert_eq!(ready.notes, "Faster search and a new Home page.");
    assert!(
        setup.whats_new().await.expect("whats new").is_none(),
        "0.1.0 has no kept notes"
    );

    // Restart to update.
    setup.install_update().await.expect("ready to install");
    assert!(setup
        .finish_update(UpdateFinish::Restart)
        .await
        .expect("finish"));
    assert_eq!(updater.installs.load(Ordering::SeqCst), 1);
    setup.close().await;
    profile.shutdown().await;
    drop(setup);

    // The program starts again as 0.2.0 on the same datastore.
    let (after, profile) = start(&data_root, &store, &feed_url, "0.2.0").await;
    let shown = after.whats_new().await.expect("whats new").expect("notes");
    assert_eq!(shown.version, "0.2.0");
    assert_eq!(shown.notes, "Faster search and a new Home page.");
    assert!(!shown.seen);
    after.whats_new_seen("0.2.0").await.expect("seen");
    assert!(
        after
            .whats_new()
            .await
            .expect("whats new")
            .expect("notes")
            .seen
    );
    // Nothing newer than 0.2.0 is offered.
    let again = Arc::new(FeedUpdater {
        http: reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client"),
        running: "0.2.0".to_owned(),
        found: Mutex::new(None),
        downloaded: Mutex::new(None),
        installs: AtomicU32::new(0),
    });
    after.register_updater(again);
    assert!(after.check_update().await.expect("check").is_none());
    after.close().await;
    profile.shutdown().await;
}
