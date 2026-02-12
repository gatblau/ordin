//! The OBS Recorder Adapter against a fake obs-websocket v5 server (ObsRecorderAdapter.md
//! *Acceptance Criteria*, one test per scenario named after it, and one per Error Table row), with
//! the real Identity Service on a temporary Journal holding the password.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ordin_identity::{
    IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig, SecretString,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, ModuleMigrations, MIGRATIONS,
};
use ordin_localisation::{Localisation, LocalisationConfig, PreferenceStore};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_types::time::Timestamp;
use ordin_types::{CodedError, RepairAction};
use serde_json::json;

use crate::detect::{Host, Platform};
use crate::fake::{FakeObs, Obs, CHALLENGE, SALT};
use crate::protocol::{authentication, version_parts, Limits};
use crate::{ObsError, ObsRecorder, ObsState};

const PASSWORD: &str = "fixture-pass";

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

async fn localisation() -> Arc<Localisation> {
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
    Localisation::start(
        LocalisationConfig {
            store: PreferenceStore::None,
            os_locales: vec!["en-GB".to_owned()],
            app_version: "1.0.0".to_owned(),
        },
        profile,
    )
    .await
    .expect("localisation")
}

/// A temporary folder removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-obs-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The desktop's Identity Service, its secrets in an encrypted file under `data`.
async fn identity(data: &Path) -> Arc<IdentityService> {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data.join("journal.db"),
        backup_dir: data.join("backups"),
    })
    .await
    .expect("open the datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry
        .register(ModuleMigrations {
            module: ordin_identity::MODULE_NAME,
            sqlite: ordin_identity::SQLITE_MIGRATIONS,
            postgres: &[],
        })
        .expect("identity");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the migrations");
    let key_file = data.join("secrets.key");
    std::fs::write(&key_file, STANDARD.encode([7u8; 32])).expect("key file");
    let config = IdentityConfig {
        program: Program::Desktop,
        data_root: data.to_path_buf(),
        keychain_service: "io.gatblau.ordin.test".to_owned(),
        secret_backend: SecretBackendConfig::EncryptedFile {
            key_source: KeySource::KeyFile(key_file),
        },
        github_app_client_id: None,
        microsoft_client_id: None,
        http: reqwest::Client::new(),
        browser: None,
    };
    Arc::new(
        IdentityService::new(config, Some(store.sqlite().expect("sqlite").clone()))
            .await
            .expect("identity"),
    )
}

struct Setup {
    platform: Platform,
    installed: bool,
    launch_fails: bool,
    saved_password: Option<&'static str>,
    limits: Limits,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            platform: Platform::Windows,
            installed: true,
            launch_fails: false,
            saved_password: Some(PASSWORD),
            limits: Limits {
                output_wait: Duration::from_millis(600),
                output_settled: Duration::from_millis(100),
                ..Limits::default()
            },
        }
    }
}

struct Env {
    dir: TempDir,
    fake: FakeObs,
    recorder: Arc<ObsRecorder>,
    launched: Arc<Mutex<Vec<PathBuf>>>,
    program: PathBuf,
}

impl Env {
    async fn start(obs: impl FnOnce(&mut Obs), setup: Setup) -> Env {
        captured_log();
        let dir = TempDir::new();
        let data = dir.0.join("data");
        std::fs::create_dir_all(&data).expect("data folder");
        let identity = identity(&data).await;
        if let Some(password) = setup.saved_password {
            identity
                .save_obs_password(SecretString::new(password.to_owned()))
                .await
                .expect("save the OBS password");
        }
        let recording = dir.0.join("recordings").join("2026-11-20 10-00-00.mkv");
        std::fs::create_dir_all(recording.parent().expect("folder")).expect("recordings folder");
        std::fs::write(&recording, b"matroska").expect("recording");
        let mut state = Obs::new("30.2.0", recording);
        state.password = Some(PASSWORD.to_owned());
        obs(&mut state);
        let fake = FakeObs::start(state).await;

        let program = dir.0.join("obs64.exe");
        let launched = Arc::new(Mutex::new(Vec::new()));
        let found = setup.installed.then(|| program.clone());
        let record = launched.clone();
        let fails = setup.launch_fails;
        let host = Host {
            platform: setup.platform,
            detect: Arc::new(move || found.clone()),
            launch: Arc::new(move |program: &Path| {
                if fails {
                    return Err(std::io::Error::other("cannot start"));
                }
                record.lock().expect("launched").push(program.to_path_buf());
                Ok(())
            }),
        };
        let recorder = ObsRecorder::with_host(identity, localisation().await, host, setup.limits);
        recorder.set_port(fake.port).expect("port");
        Env {
            dir,
            fake,
            recorder,
            launched,
            program,
        }
    }

    fn recording_path(&self) -> PathBuf {
        self.fake.obs().output_path.clone()
    }
}

/// A loopback port nothing listens on.
fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    drop(listener);
    port
}

fn tracks(on: u8) -> serde_json::Value {
    let mut tracks = serde_json::Map::new();
    for n in 1..=6u8 {
        tracks.insert(n.to_string(), serde_json::Value::Bool(n == on));
    }
    serde_json::Value::Object(tracks)
}

// Scenario: Happy path — record and stop against a fake OBS
#[tokio::test]
async fn happy_path_record_and_stop_against_a_fake_obs() {
    let env = Env::start(|_| {}, Setup::default()).await;
    let started = env.recorder.start().await.expect("start");
    assert!(started.others_heard);
    {
        let obs = env.fake.obs();
        assert_eq!(
            obs.identified_with,
            vec![Some(authentication(PASSWORD, SALT, CHALLENGE))]
        );
        assert_eq!(obs.types().last().map(String::as_str), Some("StartRecord"));
        assert!(obs.recording);
    }
    env.fake.obs().duration_ms = 61_000;
    let finished = env.recorder.stop().await.expect("stop");
    assert_eq!(finished.output_path, env.recording_path());
    assert_eq!(finished.duration_ms, 61_000);
    let obs = env.fake.obs();
    assert_eq!(obs.count("StopRecord"), 1);
    assert!(!obs.recording);
}

// Scenario: Edge — start reports OBS's recording folder
#[tokio::test]
async fn edge_start_reports_obss_recording_folder() {
    let env = Env::start(
        |obs| obs.record_directory = PathBuf::from("/fixture/recordings"),
        Setup::default(),
    )
    .await;
    let started = env.recorder.start().await.expect("start");
    assert_eq!(started.record_folder, PathBuf::from("/fixture/recordings"));
    let types = env.fake.obs().types();
    let folder = types.iter().position(|t| t == "GetRecordDirectory");
    let start = types.iter().position(|t| t == "StartRecord");
    assert!(folder.is_some() && folder < start, "{types:?}");
}

// C6: an empty folder is refused, and the person's profile and scene come back.
#[tokio::test]
async fn edge_an_empty_recording_folder_restores_and_fails() {
    let env = Env::start(
        |obs| {
            obs.record_directory = PathBuf::new();
            obs.current_profile = "Untitled".to_owned();
        },
        Setup::default(),
    )
    .await;
    assert_eq!(env.recorder.start().await, Err(ObsError::Request(0)));
    let obs = env.fake.obs();
    assert!(!obs.recording);
    assert_eq!(obs.current_profile, "Untitled");
    assert_eq!(obs.current_scene, "Scene");
}

/// The example of the obs-websocket protocol: password `supersecretpassword` with the salt and
/// challenge of ObsRecorderAdapter.md *Example* gives the string the example sends.
#[test]
fn the_authentication_string_follows_the_protocol() {
    assert_eq!(
        authentication("supersecretpassword", SALT, CHALLENGE),
        "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
    );
}

// Scenario: Edge — the meeting scene is created only when absent
#[tokio::test]
async fn edge_the_meeting_scene_is_created_only_when_absent() {
    let env = Env::start(|_| {}, Setup::default()).await;
    for _ in 0..2 {
        env.recorder.start().await.expect("start");
        env.recorder.stop().await.expect("stop");
    }
    let obs = env.fake.obs();
    assert_eq!(obs.count("CreateScene"), 1);
    assert_eq!(obs.count("CreateInput"), 2);
    assert_eq!(obs.count("CreateProfile"), 1);
    let you = &obs.inputs["You"];
    let others = &obs.inputs["Others"];
    assert_eq!(you.0, "wasapi_input_capture");
    assert_eq!(others.0, "wasapi_output_capture");
    assert_eq!(you.1, tracks(1));
    assert_eq!(others.1, tracks(2));
}

// Scenario: Edge — the person's profile and scene come back
#[tokio::test]
async fn edge_the_persons_profile_and_scene_come_back() {
    let env = Env::start(
        |obs| {
            obs.profiles = vec!["Streaming".to_owned()];
            obs.current_profile = "Streaming".to_owned();
            obs.scenes = vec!["Camera".to_owned()];
            obs.current_scene = "Camera".to_owned();
        },
        Setup::default(),
    )
    .await;
    env.recorder.start().await.expect("start");
    {
        let obs = env.fake.obs();
        assert_eq!(obs.current_profile, "Ordin meeting");
        assert_eq!(obs.current_scene, "Ordin meeting");
        let set: Vec<(&str, &str, &str)> = obs
            .parameters
            .iter()
            .map(|(c, n, v)| (c.as_str(), n.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            set,
            vec![
                ("Output", "Mode", "Advanced"),
                ("AdvOut", "RecType", "Standard"),
                ("AdvOut", "RecFormat2", "mkv"),
                ("AdvOut", "RecTracks", "3"),
                ("AdvOut", "Track1Name", "You"),
                ("AdvOut", "Track2Name", "Others"),
            ]
        );
    }
    env.recorder.stop().await.expect("stop");
    let obs = env.fake.obs();
    assert_eq!(obs.current_profile, "Streaming");
    assert_eq!(obs.current_scene, "Camera");
}

// Scenario: Error — three distinct failures
#[tokio::test]
async fn error_three_distinct_failures() {
    let not_installed = Env::start(
        |_| {},
        Setup {
            installed: false,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(not_installed.recorder.state().await, ObsState::NotInstalled);
    assert!(not_installed.fake.obs().identified_with.is_empty());

    let refused_port = Env::start(|_| {}, Setup::default()).await;
    refused_port.recorder.set_port(closed_port()).expect("port");
    assert_eq!(refused_port.recorder.state().await, ObsState::NotReachable);

    let wrong_password = Env::start(
        |_| {},
        Setup {
            saved_password: Some("not-the-password"),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(
        wrong_password.recorder.state().await,
        ObsState::PasswordRefused
    );
}

// Scenario: Edge — older OBS on macOS records the microphone only
#[tokio::test]
async fn edge_older_obs_on_macos_records_the_microphone_only() {
    let mac = Setup {
        platform: Platform::MacOs,
        ..Setup::default()
    };
    let env = Env::start(|obs| obs.obs_version = "29.1.3".to_owned(), mac).await;
    let started = env.recorder.start().await.expect("start");
    assert!(!started.others_heard);
    {
        let obs = env.fake.obs();
        assert_eq!(obs.count("CreateInput"), 1);
        assert_eq!(obs.inputs["You"].0, "coreaudio_input_capture");
        assert_eq!(obs.inputs["You"].1, tracks(1));
        assert!(!obs.inputs.contains_key("Others"));
    }

    // From OBS 30 the computer's sound is heard on macOS too.
    let mac = Setup {
        platform: Platform::MacOs,
        ..Setup::default()
    };
    let env = Env::start(|obs| obs.obs_version = "30.0.0".to_owned(), mac).await;
    assert!(env.recorder.start().await.expect("start").others_heard);
    assert_eq!(env.fake.obs().inputs["Others"].0, "sck_audio_capture");
}

/// Collects every log line written while it is the default subscriber.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The log of this test binary: a process-wide subscriber, installed by the first test that
/// starts an environment. A per-thread subscriber misses lines while other tests run in parallel,
/// as they settle the log call sites' interest without it.
fn captured_log() -> &'static Captured {
    static LOG: std::sync::OnceLock<Captured> = std::sync::OnceLock::new();
    LOG.get_or_init(|| {
        let log = Captured::default();
        let writer = log.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("the only subscriber");
        log
    })
}

/// True when any file under `dir`, read as bytes, holds `needle`.
fn any_file_holds(dir: &Path, needle: &[u8]) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            any_file_holds(&path, needle)
        } else {
            std::fs::read(&path)
                .map(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
                .unwrap_or(false)
        }
    })
}

// Scenario: Edge — the password never appears
// Every test's log lines go to the one capture, so this also checks the lines of the others.
#[tokio::test]
async fn edge_the_password_never_appears() {
    let log = captured_log();
    let env = Env::start(|_| {}, Setup::default()).await;
    assert!(matches!(env.recorder.state().await, ObsState::Ready { .. }));
    env.recorder.start().await.expect("start");
    env.recorder.stop().await.expect("stop");
    let refused = Env::start(
        |_| {},
        Setup {
            saved_password: Some("not-the-password"),
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(
        refused.recorder.start().await.unwrap_err(),
        ObsError::PasswordRefused
    );

    let text = String::from_utf8_lossy(&log.0.lock().expect("log")).into_owned();
    assert!(text.contains("obs.recording_started"), "{text}");
    assert!(text.contains("obs.recording_stopped"), "{text}");
    let recording = env.recording_path();
    let never = [
        PASSWORD.to_owned(),
        "not-the-password".to_owned(),
        authentication(PASSWORD, SALT, CHALLENGE),
        SALT.to_owned(),
        CHALLENGE.to_owned(),
        recording.to_string_lossy().into_owned(),
        "2026-11-20 10-00-00".to_owned(),
    ];
    for secret in &never {
        assert!(!text.contains(secret.as_str()), "the log holds {secret}");
    }
    for dir in [&env.dir.0, &refused.dir.0] {
        let data = dir.join("data");
        assert!(!any_file_holds(&data, PASSWORD.as_bytes()));
        assert!(!any_file_holds(&data, b"not-the-password"));
    }
}

// Error Table: OBS asks for a password and none is saved.
#[tokio::test]
async fn error_table_obs_password_needed() {
    let env = Env::start(
        |_| {},
        Setup {
            saved_password: None,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(env.recorder.state().await, ObsState::PasswordNeeded);
    assert_eq!(
        env.recorder.start().await.unwrap_err(),
        ObsError::PasswordNeeded
    );
    // Identify was never sent.
    assert!(env.fake.obs().identified_with.is_empty());
}

// Error Table: no password is asked for, so none is read or sent.
#[tokio::test]
async fn edge_obs_without_a_password() {
    let env = Env::start(
        |obs| obs.password = None,
        Setup {
            saved_password: None,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(
        env.recorder.state().await,
        ObsState::Ready {
            obs_version: "30.2.0".to_owned(),
            others_heard: true
        }
    );
    assert_eq!(env.fake.obs().identified_with, vec![None]);
}

// Error Table: OBS older than 28, or another protocol.
#[tokio::test]
async fn error_table_obs_version_unsupported() {
    let old = Env::start(
        |obs| obs.obs_version = "27.2.4".to_owned(),
        Setup::default(),
    )
    .await;
    assert_eq!(
        old.recorder.state().await,
        ObsState::VersionUnsupported {
            obs_version: "27.2.4".to_owned()
        }
    );
    assert_eq!(
        old.recorder.start().await.unwrap_err(),
        ObsError::VersionUnsupported
    );
    assert_eq!(old.fake.obs().count("StartRecord"), 0);

    let other_protocol = Env::start(|obs| obs.rpc_version = 2, Setup::default()).await;
    assert_eq!(
        other_protocol.recorder.state().await,
        ObsState::VersionUnsupported {
            obs_version: String::new()
        }
    );
}

// Error Table: OBS is already recording.
#[tokio::test]
async fn error_table_obs_already_recording() {
    let env = Env::start(
        |obs| {
            obs.recording = true;
            obs.duration_ms = 5_000;
        },
        Setup::default(),
    )
    .await;
    let before = Timestamp::now().unix_millis();
    match env.recorder.state().await {
        ObsState::Recording {
            started_at,
            others_heard,
        } => {
            assert!(others_heard);
            let started = started_at.unix_millis();
            assert!(
                started <= before - 4_000 && started >= before - 7_000,
                "{started}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        env.recorder.start().await.unwrap_err(),
        ObsError::AlreadyRecording
    );
    let obs = env.fake.obs();
    assert_eq!(obs.count("StartRecord"), 0);
    assert_eq!(obs.count("SetCurrentProfile"), 0);
}

// Error Table: stop with no recording running.
#[tokio::test]
async fn error_table_obs_not_recording() {
    let env = Env::start(|_| {}, Setup::default()).await;
    assert_eq!(
        env.recorder.stop().await.unwrap_err(),
        ObsError::NotRecording
    );
    assert_eq!(env.fake.obs().count("StopRecord"), 0);
}

// Error Table: OBS refused a request; the person's profile and scene come back.
#[tokio::test]
async fn error_table_obs_request_failed() {
    let env = Env::start(
        |obs| {
            obs.refuse.insert("StartRecord".to_owned(), 702);
        },
        Setup::default(),
    )
    .await;
    let error = env.recorder.start().await.unwrap_err();
    assert_eq!(error, ObsError::Request(702));
    assert_eq!(error.code(), "OBS_REQUEST_FAILED");
    let obs = env.fake.obs();
    assert_eq!(obs.current_profile, "Untitled");
    assert_eq!(obs.current_scene, "Scene");
}

// Error Table: no answer within the limit.
#[tokio::test]
async fn error_table_obs_timeout() {
    let env = Env::start(
        |obs| {
            obs.silent.insert("StartRecord".to_owned());
        },
        Setup {
            limits: Limits {
                request: Duration::from_millis(300),
                ..Limits::default()
            },
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(env.recorder.start().await.unwrap_err(), ObsError::Timeout);
    let obs = env.fake.obs();
    assert_eq!(obs.current_profile, "Untitled");
    assert_eq!(obs.current_scene, "Scene");
}

// Error Table: the profile or scene could not be prepared.
#[tokio::test]
async fn error_table_obs_scene_failed() {
    let env = Env::start(
        |obs| {
            obs.refuse.insert("CreateScene".to_owned(), 600);
        },
        Setup::default(),
    )
    .await;
    assert_eq!(
        env.recorder.start().await.unwrap_err(),
        ObsError::SceneFailed
    );
    let obs = env.fake.obs();
    assert_eq!(obs.count("StartRecord"), 0);
    assert_eq!(obs.current_profile, "Untitled");
    assert_eq!(obs.current_scene, "Scene");
}

// Error Table: the recorded file is not where OBS said.
#[tokio::test]
async fn error_table_obs_output_missing() {
    let env = Env::start(|_| {}, Setup::default()).await;
    env.fake.obs().output_path = env.dir.0.join("nowhere.mkv");
    env.recorder.start().await.expect("start");
    assert_eq!(
        env.recorder.stop().await.unwrap_err(),
        ObsError::OutputMissing
    );
    let obs = env.fake.obs();
    assert!(!obs.recording);
    assert_eq!(obs.current_profile, "Untitled");
}

// Error Table: port outside 1024–65535.
#[tokio::test]
async fn error_table_obs_port_invalid() {
    let env = Env::start(|_| {}, Setup::default()).await;
    for port in [0, 80, 1023] {
        assert_eq!(
            env.recorder.set_port(port),
            Err(ObsError::PortInvalid),
            "{port}"
        );
    }
    // A refused port leaves the earlier one in use.
    assert!(matches!(env.recorder.state().await, ObsState::Ready { .. }));
    for port in [1024, 4455, 65535] {
        assert_eq!(env.recorder.set_port(port), Ok(()), "{port}");
    }
}

// Error Table: nothing answers on the port; OBS closed during a recording (D3).
#[tokio::test]
async fn error_table_obs_not_reachable() {
    let env = Env::start(|_| {}, Setup::default()).await;
    env.recorder.start().await.expect("start");
    env.recorder.set_port(closed_port()).expect("port");
    assert_eq!(
        env.recorder.stop().await.unwrap_err(),
        ObsError::NotReachable
    );
    assert_eq!(
        env.recorder.start().await.unwrap_err(),
        ObsError::NotReachable
    );
}

// Error Table: OBS not installed.
#[tokio::test]
async fn error_table_obs_not_installed() {
    let env = Env::start(
        |_| {},
        Setup {
            installed: false,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(
        env.recorder.open_obs().await.unwrap_err(),
        ObsError::NotInstalled
    );
    assert!(env.launched.lock().expect("launched").is_empty());
}

#[tokio::test]
async fn open_obs_starts_the_detected_program() {
    let env = Env::start(|_| {}, Setup::default()).await;
    env.recorder.open_obs().await.expect("open");
    assert_eq!(
        *env.launched.lock().expect("launched"),
        vec![env.program.clone()]
    );
    // Opening does not connect.
    assert!(env.fake.obs().identified_with.is_empty());

    let failing = Env::start(
        |_| {},
        Setup {
            launch_fails: true,
            ..Setup::default()
        },
    )
    .await;
    assert_eq!(
        failing.recorder.open_obs().await.unwrap_err(),
        ObsError::NotReachable
    );
}

#[test]
fn every_error_has_its_code_retry_flag_and_repair() {
    let table = [
        (ObsError::NotInstalled, "OBS_NOT_INSTALLED", false),
        (ObsError::NotReachable, "OBS_NOT_REACHABLE", true),
        (ObsError::PasswordNeeded, "OBS_PASSWORD_NEEDED", false),
        (ObsError::PasswordRefused, "OBS_PASSWORD_REFUSED", false),
        (
            ObsError::VersionUnsupported,
            "OBS_VERSION_UNSUPPORTED",
            false,
        ),
        (ObsError::AlreadyRecording, "OBS_ALREADY_RECORDING", false),
        (ObsError::NotRecording, "OBS_NOT_RECORDING", false),
        (ObsError::Request(702), "OBS_REQUEST_FAILED", true),
        (ObsError::Timeout, "OBS_TIMEOUT", true),
        (ObsError::SceneFailed, "OBS_SCENE_FAILED", true),
        (ObsError::OutputMissing, "OBS_OUTPUT_MISSING", false),
        (ObsError::PortInvalid, "OBS_PORT_INVALID", false),
    ];
    assert_eq!(table.len(), crate::ALL_CODES.len());
    for (error, code, retryable) in table {
        assert_eq!(error.code(), code);
        assert!(crate::ALL_CODES.contains(&code));
        assert_eq!(error.retryable(), retryable, "{code}");
        assert_eq!(
            error.repair(),
            retryable.then_some(RepairAction::TryAgain),
            "{code}"
        );
    }
}

#[test]
fn versions_compare_by_their_numbers() {
    for (text, parts) in [
        ("30.2.0", (30, 2, 0)),
        ("29.1.3", (29, 1, 3)),
        ("30.0.0-rc1", (30, 0, 0)),
        ("28", (28, 0, 0)),
        ("", (0, 0, 0)),
        ("beta", (0, 0, 0)),
    ] {
        assert_eq!(version_parts(text), parts, "{text}");
    }
}

#[test]
fn the_states_serialise_with_their_tag() {
    let state = ObsState::Ready {
        obs_version: "30.2.0".to_owned(),
        others_heard: true,
    };
    assert_eq!(
        serde_json::to_value(&state).expect("json"),
        json!({ "state": "ready", "obs_version": "30.2.0", "others_heard": true })
    );
    assert_eq!(
        serde_json::to_value(ObsState::NotInstalled).expect("json"),
        json!({ "state": "not_installed" })
    );
}

// The real OBS on the Windows test machine (plan 07 Phase 9: FU-010, C-111).
#[cfg(windows)]
mod real_obs;

// ObsRecorderAdapter.md OBS-PRO-1 to OBS-PRO-3: the fake OBS, like real OBS, builds its recording
// output from a profile's parameters only when the profile is selected.

fn loaded(obs: &crate::fake::Obs) -> Vec<(String, String)> {
    obs.loaded_parameters
        .iter()
        .map(|(c, n, v)| (format!("{c}/{n}"), v.clone()))
        .collect()
}

// Scenario: The recording settings are in force when the recording starts
// spec: ordin-desktop/components/ObsRecorderAdapter#OBS-PRO-1
#[tokio::test]
async fn scenario_the_recording_settings_are_in_force_when_the_recording_starts() {
    let env = Env::start(|_| {}, Setup::default()).await;
    env.recorder.start().await.expect("start");
    {
        let obs = env.fake.obs();
        assert_eq!(obs.current_profile, "Ordin meeting");
        assert_eq!(
            obs.selected_profiles,
            vec![
                "Ordin meeting",
                "Ordin meeting",
                "Untitled",
                "Ordin meeting"
            ]
        );
        let loaded = loaded(&obs);
        assert!(loaded.contains(&("Output/Mode".to_owned(), "Advanced".to_owned())));
        assert!(loaded.contains(&("AdvOut/RecFormat2".to_owned(), "mkv".to_owned())));
        assert!(loaded.contains(&("AdvOut/RecTracks".to_owned(), "3".to_owned())));
    }
    env.recorder.stop().await.expect("stop");
    assert_eq!(env.fake.obs().current_profile, "Untitled");
}

// Scenario: The person was already on Ordin's profile
// spec: ordin-desktop/components/ObsRecorderAdapter#OBS-PRO-1
#[tokio::test]
async fn scenario_the_person_was_already_on_ordins_profile() {
    let env = Env::start(
        |obs| {
            obs.profiles = vec!["Ordin meeting".to_owned(), "Untitled".to_owned()];
            obs.current_profile = "Ordin meeting".to_owned();
        },
        Setup::default(),
    )
    .await;
    env.recorder.start().await.expect("start");
    let obs = env.fake.obs();
    assert_eq!(
        obs.selected_profiles,
        vec!["Ordin meeting", "Untitled", "Ordin meeting"]
    );
    assert!(loaded(&obs).contains(&("AdvOut/RecFormat2".to_owned(), "mkv".to_owned())));
}

// Scenario: No other profile to switch through
// spec: ordin-desktop/components/ObsRecorderAdapter#OBS-PRO-2
#[tokio::test]
async fn scenario_no_other_profile_to_switch_through() {
    let env = Env::start(
        |obs| {
            obs.profiles = vec!["Ordin meeting".to_owned()];
            obs.current_profile = "Ordin meeting".to_owned();
        },
        Setup::default(),
    )
    .await;
    env.recorder.start().await.expect("start");
    let obs = env.fake.obs();
    assert!(obs.recording);
    assert_eq!(obs.selected_profiles, vec!["Ordin meeting"]);
}

// OBS-PRO-3: a refused reload is a failure of C3, which restores the person's profile and scene.
// spec: ordin-desktop/components/ObsRecorderAdapter#OBS-PRO-3
#[tokio::test]
async fn error_a_refused_reload_is_scene_failed() {
    let env = Env::start(
        |obs| {
            obs.profiles = vec!["Streaming".to_owned(), "Ordin meeting".to_owned()];
            obs.current_profile = "Streaming".to_owned();
            obs.scenes = vec!["Camera".to_owned()];
            obs.current_scene = "Camera".to_owned();
            obs.refuse.insert("SetCurrentProfile".to_owned(), 600);
        },
        Setup::default(),
    )
    .await;
    assert_eq!(
        env.recorder.start().await.unwrap_err(),
        ObsError::SceneFailed
    );
    let obs = env.fake.obs();
    assert!(!obs.recording);
    assert_eq!(obs.current_profile, "Streaming");
    assert_eq!(obs.current_scene, "Camera");
}

// ObsRecorderAdapter.md OBS-STOP-1: OBS answers StopRecord before it has closed the file, so
// `stop` waits until the file's size has stayed the same.

// Scenario: Stop waits for OBS to finish the file
// spec: ordin-desktop/components/ObsRecorderAdapter#OBS-STOP-1
#[tokio::test]
async fn scenario_stop_waits_for_obs_to_finish_the_file() {
    let env = Env::start(|_| {}, Setup::default()).await;
    let path = env.recording_path();
    env.recorder.start().await.expect("start");
    std::fs::write(&path, b"").expect("empty file");
    // OBS still writing: one block every 30 ms for 300 ms after StopRecord.
    let writing = {
        let path = path.clone();
        tokio::spawn(async move {
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(30)).await;
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .expect("open");
                std::io::Write::write_all(&mut file, &[7u8; 1024]).expect("write");
            }
        })
    };
    let finished = env.recorder.stop().await.expect("stop");
    assert!(writing.is_finished(), "stop returned while OBS was writing");
    assert_eq!(
        std::fs::metadata(&finished.output_path)
            .expect("file")
            .len(),
        10 * 1024
    );
}

// Scenario: A file that stays empty is missing
// spec: ordin-desktop/components/ObsRecorderAdapter#OBS-STOP-1
#[tokio::test]
async fn error_a_file_that_stays_empty_is_output_missing() {
    let env = Env::start(|_| {}, Setup::default()).await;
    let path = env.recording_path();
    env.recorder.start().await.expect("start");
    std::fs::write(&path, b"").expect("empty file");
    assert_eq!(
        env.recorder.stop().await.unwrap_err(),
        ObsError::OutputMissing
    );
}
