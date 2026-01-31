//! X2: one test per Gherkin scenario of the ADR-114 amendment (releases and updates) and the
//! ADR-072 amendment (the install choice), named after it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use ordin_types::CodedError;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::store;
use crate::types::{
    CheckKind, CheckOutcome, CheckTrigger, InstallKind, ServerRunMode, ServerUpdateOutcome,
    UpdateFinish, UpdateState, UpdaterError,
};

const PLATFORM: &str = "linux-x86_64";
const KEY_ID: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

/// The public key as Tauri's updater carries it: base64 of minisign's key file.
pub(crate) fn public_key() -> String {
    let mut bin = b"Ed".to_vec();
    bin.extend_from_slice(&KEY_ID);
    bin.extend_from_slice(&signing_key().verifying_key().to_bytes());
    b64(format!("untrusted comment: minisign public key\n{}\n", b64(&bin)).as_bytes())
}

/// A minisign (legacy) signature of `bytes`, as base64 of the `.sig` file's text.
pub(crate) fn sign(bytes: &[u8]) -> String {
    let key = signing_key();
    let signature = key.sign(bytes).to_bytes();
    let mut bin = b"Ed".to_vec();
    bin.extend_from_slice(&KEY_ID);
    bin.extend_from_slice(&signature);
    let trusted = "timestamp:0\tfile:ordin-server.tar.gz";
    let mut global = signature.to_vec();
    global.extend_from_slice(trusted.as_bytes());
    let global = key.sign(&global).to_bytes();
    b64(format!(
        "untrusted comment: signature from ordin\n{}\ntrusted comment: {trusted}\n{}\n",
        b64(&bin),
        b64(&global)
    )
    .as_bytes())
}

/// What the archive holds.
pub(crate) enum Hook {
    File,
    Symlink,
    Missing,
}

/// A `.tar.gz` with `bin/ordin-server` printing `server_version` and `bin/ordin-hook`.
pub(crate) fn archive(server_version: &str, hook: Hook) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut dir = tar::Header::new_gnu();
    dir.set_entry_type(tar::EntryType::Directory);
    dir.set_mode(0o755);
    dir.set_size(0);
    builder
        .append_data(&mut dir, "bin/", std::io::empty())
        .expect("dir");
    let mut add = |name: &str, body: &str| {
        let data = body.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_entry_type(tar::EntryType::Regular);
        builder.append_data(&mut header, name, data).expect("file");
    };
    add(
        "bin/ordin-server",
        &format!("#!/bin/sh\necho '{server_version} (ordin-server)'\n"),
    );
    add("README.md", "notes\n");
    match hook {
        Hook::File => add("bin/ordin-hook", "#!/bin/sh\necho hook\n"),
        Hook::Missing => {}
        Hook::Symlink => {
            let mut link = tar::Header::new_gnu();
            link.set_entry_type(tar::EntryType::Symlink);
            link.set_size(0);
            builder
                .append_link(&mut link, "bin/ordin-hook", "/etc/passwd")
                .expect("link");
        }
    }
    let tar = builder.into_inner().expect("tar");
    use std::io::Write;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).expect("gz");
    gz.finish().expect("gz")
}

/// A personal server with its two programs in `<dir>/programs`, a feed stand-in and the test key.
pub(crate) struct Server {
    pub env: Env,
    pub mock: MockServer,
    pub programs: PathBuf,
    pub platform: Arc<FakePlatform>,
}

impl Server {
    pub async fn new(version: &'static str, mode: ServerRunMode) -> Server {
        let env = Env::with(Options {
            program: SetupProgram::PersonalServer,
            version,
            ..Options::default()
        })
        .await;
        let programs = env.dir.path().join("programs");
        std::fs::create_dir_all(&programs).expect("programs");
        std::fs::write(programs.join("ordin-server"), "old server").expect("server");
        std::fs::write(programs.join("ordin-hook"), "old hook").expect("hook");
        let platform = Arc::new(FakePlatform {
            mode,
            dir: programs.clone(),
            key: PLATFORM,
            restart: Mutex::new(Ok(())),
            restarts: AtomicU32::new(0),
        });
        env.setup.register_server_platform(platform.clone());
        env.tuning().update_key = Some(public_key());
        let mock = MockServer::start().await;
        let feed_url = format!("{}/latest.json", mock.uri());
        env.profile.edit(|d| d.updates = Some(feed(&feed_url)));
        Server {
            env,
            mock,
            programs,
            platform,
        }
    }

    /// The feed offering `version` with `archive` for this platform (signed with `signature`).
    pub async fn offer(
        &self,
        version: &str,
        archive: Vec<u8>,
        signature: Option<String>,
        url: Option<String>,
    ) {
        let signature = signature.unwrap_or_else(|| sign(&archive));
        let url = url.unwrap_or_else(|| format!("{}/ordin-server.tar.gz", self.mock.uri()));
        Mock::given(method("GET"))
            .and(path("/latest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "version": version,
                "notes": format!("What changed in {version}"),
                "pub_date": "2026-01-10T08:00:00Z",
                "platforms": { PLATFORM: { "url": url, "signature": signature } }
            })))
            .mount(&self.mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/ordin-server.tar.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .mount(&self.mock)
            .await;
    }

    pub fn file(&self, name: &str) -> String {
        std::fs::read_to_string(self.programs.join(name)).unwrap_or_default()
    }

    pub fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(&self.programs)
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".update"))
            .collect()
    }

    pub async fn row(&self) -> Option<(String, String)> {
        store::server_update_row(&self.env.pool)
            .await
            .expect("row")
            .map(|(v, s, _)| (v, s))
    }
}

async fn desktop(updater: Arc<FakeUpdater>) -> Env {
    let env = Env::new().await;
    env.profile
        .edit(|d| d.updates = Some(feed("https://updates.example/ordin/latest.json")));
    env.setup.register_updater(updater);
    env
}

async fn wait_ready(env: &Env) {
    let task = lock(&env.setup.inner.download_task).take();
    if let Some(task) = task {
        task.await.expect("download task");
    }
}

// ---------------------------------------------------------------- ADR-114, desktop

// Scenario: Happy path — an update is downloaded without a question and offered as ready
#[tokio::test]
async fn happy_path_an_update_is_downloaded_without_a_question_and_offered_as_ready() {
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.download_delay) = Duration::from_millis(100);
    let env = desktop(updater.clone()).await;
    env.setup.inner.clone().timer_update_check().await;
    let offer = env.setup.update_offer().expect("offer");
    assert_eq!(offer.version, "0.2.0");
    assert_eq!(offer.state, UpdateState::Downloading);
    wait_ready(&env).await;
    assert_eq!(
        env.setup.update_offer().expect("offer").state,
        UpdateState::Ready
    );
    let kept = store::whats_new(&env.pool, "0.2.0").await.expect("row");
    assert_eq!(kept, Some(("Notes for 0.2.0".to_owned(), false)));
}

// Scenario: Happy path — the person asks for a check
#[tokio::test]
async fn happy_path_the_person_asks_for_a_check() {
    let env = desktop(FakeUpdater::offering("0.2.0")).await;
    let offer = env
        .setup
        .check_update()
        .await
        .expect("check")
        .expect("offer");
    assert_eq!(offer.version, "0.2.0");
}

// Scenario: Edge — two checks at once run one check
#[tokio::test]
async fn edge_two_checks_at_once_run_one_check() {
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.check_delay) = Duration::from_millis(200);
    let env = desktop(updater.clone()).await;
    let background = env.setup.inner.clone().timer_update_check();
    let asked = env.setup.check_update();
    let ((), asked) = tokio::join!(background, asked);
    assert_eq!(asked.expect("check").expect("offer").version, "0.2.0");
    assert_eq!(updater.checks.load(Ordering::SeqCst), 1);
}

// Scenario: Error — the person's check cannot reach the feed
#[tokio::test]
async fn error_the_persons_check_cannot_reach_the_feed() {
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.offer) = Err(UpdaterError::Unreachable);
    let env = desktop(updater).await;
    let error = env.setup.check_update().await.expect_err("unreachable");
    assert_eq!(error.code(), "SETUP_UPDATE_UNREACHABLE");
    assert!(error.retryable());
}

// Scenario: Error — a download with a bad signature is dropped
#[tokio::test]
async fn error_a_download_with_a_bad_signature_is_dropped() {
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.download) = Err(UpdaterError::SignatureInvalid);
    let env = desktop(updater).await;
    env.setup.check_update().await.expect("check");
    wait_ready(&env).await;
    assert_eq!(env.setup.update_offer(), None);
}

// Scenario: Error — Restart to update before the download is ready
#[tokio::test]
async fn error_restart_to_update_before_the_download_is_ready() {
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.download_delay) = Duration::from_secs(5);
    let env = desktop(updater).await;
    env.setup.check_update().await.expect("check");
    assert_eq!(
        env.setup.update_offer().expect("offer").state,
        UpdateState::Downloading
    );
    assert_eq!(code(env.setup.install_update().await), "SETUP_UPDATE_NONE");
}

async fn ready(updater: Arc<FakeUpdater>) -> Env {
    let env = desktop(updater).await;
    env.setup.check_update().await.expect("check");
    wait_ready(&env).await;
    assert_eq!(
        env.setup.update_offer().expect("offer").state,
        UpdateState::Ready
    );
    env
}

// Scenario: Happy path — Restart to update
#[tokio::test]
async fn happy_path_restart_to_update() {
    let updater = FakeUpdater::offering("0.2.0");
    let env = ready(updater.clone()).await;
    env.setup.install_update().await.expect("install");
    assert!(env
        .setup
        .finish_update(UpdateFinish::Restart)
        .await
        .expect("finish"));
    assert_eq!(updater.restarts.load(Ordering::SeqCst), 1);
}

// Scenario: Happy path — quitting installs a ready update
#[tokio::test]
async fn happy_path_quitting_installs_a_ready_update() {
    let updater = FakeUpdater::offering("0.2.0");
    let env = ready(updater.clone()).await;
    assert!(env
        .setup
        .finish_update(UpdateFinish::OnExit)
        .await
        .expect("finish"));
    assert_eq!(updater.on_exit.load(Ordering::SeqCst), 1);
    assert_eq!(updater.restarts.load(Ordering::SeqCst), 0);
}

// Scenario: Edge — quitting with nothing ready
#[tokio::test]
async fn edge_quitting_with_nothing_ready() {
    let updater = FakeUpdater::offering("0.2.0");
    let env = desktop(updater.clone()).await;
    assert!(!env
        .setup
        .finish_update(UpdateFinish::OnExit)
        .await
        .expect("finish"));
    assert_eq!(updater.on_exit.load(Ordering::SeqCst), 0);
}

// Scenario: Error — the install fails
#[tokio::test]
async fn error_the_install_fails() {
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.install) = Err(UpdaterError::InstallFailed);
    let env = ready(updater).await;
    let error = env
        .setup
        .finish_update(UpdateFinish::Restart)
        .await
        .expect_err("failed");
    assert_eq!(error.code(), "SETUP_UPDATE_INSTALL_FAILED");
    assert_eq!(
        env.setup.update_offer().expect("offer").state,
        UpdateState::Ready
    );
}

// Scenario: Happy path — What's new once, then in About
#[tokio::test]
async fn happy_path_whats_new_once_then_in_about() {
    let env = Env::with(Options {
        version: "0.2.0",
        ..Options::default()
    })
    .await;
    store::keep_whats_new(
        &env.pool,
        "0.2.0",
        "Faster search",
        ordin_types::time::Timestamp::now(),
    )
    .await
    .expect("keep");
    let first = env.setup.whats_new().await.expect("read").expect("row");
    assert_eq!((first.version.as_str(), first.seen), ("0.2.0", false));
    env.setup.whats_new_seen("0.2.0").await.expect("seen");
    let again = env.setup.whats_new().await.expect("read").expect("row");
    assert!(again.seen);
}

// Scenario: Edge — a fresh install keeps no notes
#[tokio::test]
async fn edge_a_fresh_install_keeps_no_notes() {
    let env = Env::new().await;
    assert_eq!(env.setup.whats_new().await.expect("read"), None);
}

// ---------------------------------------------------------------- ADR-114, personal server

// Scenario: Happy path — the owner updates a personal server
// Unix only: the personal server replaces itself under systemd on Linux; the desktop on Windows never does.
#[cfg(unix)]
#[tokio::test]
async fn happy_path_the_owner_updates_a_personal_server() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer("0.5.0", archive("0.5.0", Hook::File), None, None)
        .await;
    let outcome = s.env.setup.run_server_update().await.expect("update");
    assert_eq!(
        outcome,
        ServerUpdateOutcome::Restarting {
            version: "0.5.0".to_owned()
        }
    );
    assert_eq!(s.file("ordin-server.previous"), "old server");
    assert_eq!(s.file("ordin-hook.previous"), "old hook");
    assert!(s.file("ordin-server").contains("0.5.0"));
    assert!(s.file("ordin-hook").contains("hook"));
    assert_eq!(s.platform.restarts.load(Ordering::SeqCst), 1);
    assert!(s.leftovers().is_empty());
    assert_eq!(
        s.row().await,
        Some(("0.5.0".to_owned(), "restarting".to_owned()))
    );
}

// Scenario: Happy path — a paired desktop asks the server to update
// Unix only: the personal server replaces itself under systemd on Linux; the desktop on Windows never does.
#[cfg(unix)]
#[tokio::test]
async fn happy_path_a_paired_desktop_asks_the_server_to_update() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer("0.5.0", archive("0.5.0", Hook::File), None, None)
        .await;
    let update = s
        .env
        .setup
        .server_update()
        .await
        .expect("read")
        .expect("newer");
    assert_eq!(update.version, "0.5.0");
    assert!(update.self_update);
    s.env.setup.update_server("0.5.0").await.expect("accepted");
    let task = lock(&s.env.setup.inner.server_task).take().expect("task");
    task.await.expect("task");
    assert_eq!(s.platform.restarts.load(Ordering::SeqCst), 1);
}

// Scenario: Edge — after the restart the update is recorded as done
// Unix only: the personal server replaces itself under systemd on Linux; the desktop on Windows never does.
#[cfg(unix)]
#[tokio::test]
async fn edge_after_the_restart_the_update_is_recorded_as_done() {
    let s = Server::new("0.5.0", ServerRunMode::UserUnit).await;
    store::claim_server_update(&s.env.pool, "0.5.0", ordin_types::time::Timestamp::now())
        .await
        .expect("claim");
    store::set_server_update(
        &s.env.pool,
        "restarting",
        None,
        ordin_types::time::Timestamp::now(),
    )
    .await
    .expect("restarting");
    s.env
        .setup
        .run_checks(CheckTrigger::Start)
        .await
        .expect("checks");
    assert_eq!(s.row().await, Some(("0.5.0".to_owned(), "done".to_owned())));
}

// Scenario: Error — the server commands on the desktop
#[tokio::test]
async fn error_the_server_commands_on_the_desktop() {
    let env = Env::new().await;
    assert_eq!(
        code(env.setup.server_update().await),
        "SETUP_SELF_UPDATE_UNAVAILABLE"
    );
}

// Scenario: Error — a server in a container cannot replace itself
#[tokio::test]
async fn error_a_server_in_a_container_cannot_replace_itself() {
    let s = Server::new("0.4.0", ServerRunMode::Container).await;
    s.offer("0.5.0", archive("0.5.0", Hook::File), None, None)
        .await;
    let update = s
        .env
        .setup
        .server_update()
        .await
        .expect("read")
        .expect("newer");
    assert_eq!(update.version, "0.5.0");
    assert!(!update.self_update);
    assert_eq!(
        code(s.env.setup.update_server("0.5.0").await),
        "SETUP_SELF_UPDATE_UNAVAILABLE"
    );
}

// Scenario: Error — no archive for this architecture
#[tokio::test]
async fn error_no_archive_for_this_architecture() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "version": "0.5.0", "notes": "", "platforms": {}
        })))
        .mount(&s.mock)
        .await;
    assert_eq!(
        code(s.env.setup.run_server_update().await),
        "SETUP_UPDATE_NO_ARCHIVE"
    );
    assert_eq!(s.file("ordin-server"), "old server");
    assert_eq!(s.row().await.map(|r| r.1).as_deref(), Some("failed"));
}

// Scenario: Error — a server archive with a bad signature
#[tokio::test]
async fn error_a_server_archive_with_a_bad_signature() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer(
        "0.5.0",
        archive("0.5.0", Hook::File),
        Some(sign(b"another file")),
        None,
    )
    .await;
    assert_eq!(
        code(s.env.setup.run_server_update().await),
        "SETUP_UPDATE_SIGNATURE_INVALID"
    );
    assert!(s.leftovers().is_empty());
    assert_eq!(s.file("ordin-server"), "old server");
}

// Scenario: Error — the signed archive is not the version offered
#[tokio::test]
async fn error_the_signed_archive_is_not_the_version_offered() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer("0.5.0", archive("0.4.9", Hook::File), None, None)
        .await;
    assert_eq!(
        code(s.env.setup.run_server_update().await),
        "SETUP_UPDATE_FEED_INVALID"
    );
    assert_eq!(s.file("ordin-server"), "old server");
    assert_eq!(s.file("ordin-hook"), "old hook");
}

// Scenario: Error — replacing a program fails
// Unix only: the personal server replaces itself under systemd on Linux; the desktop on Windows never does.
#[cfg(unix)]
#[tokio::test]
async fn error_replacing_a_program_fails() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    // `ordin-server.previous` is a folder that is not empty, so keeping the old server fails
    // after `ordin-hook` was replaced.
    let blocker = s.programs.join("ordin-server.previous");
    std::fs::create_dir_all(blocker.join("x")).expect("blocker");
    s.offer("0.5.0", archive("0.5.0", Hook::File), None, None)
        .await;
    let error = s.env.setup.run_server_update().await.expect_err("failed");
    assert_eq!(error.code(), "SETUP_UPDATE_REPLACE_FAILED");
    assert!(error.retryable());
    assert_eq!(s.file("ordin-hook"), "old hook");
    assert_eq!(s.file("ordin-server"), "old server");
}

// Scenario: Error — a second update while one runs
#[tokio::test]
async fn error_a_second_update_while_one_runs() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer("0.5.0", archive("0.5.0", Hook::File), None, None)
        .await;
    let five_minutes_ago = ordin_types::time::Timestamp(
        ordin_types::time::Timestamp::now().0 - chrono::Duration::minutes(5),
    );
    store::claim_server_update(&s.env.pool, "0.5.0", five_minutes_ago)
        .await
        .expect("claim");
    assert_eq!(
        code(s.env.setup.update_server("0.5.0").await),
        "SETUP_UPDATE_IN_PROGRESS"
    );
}

// Scenario: Error — a newer version came out meanwhile
#[tokio::test]
async fn error_a_newer_version_came_out_meanwhile() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer("0.5.1", archive("0.5.1", Hook::File), None, None)
        .await;
    assert_eq!(
        code(s.env.setup.update_server("0.5.0").await),
        "SETUP_UPDATE_VERSION_CHANGED"
    );
    assert_eq!(s.row().await, None, "the claim is released");
}

// Scenario: Error — systemd refuses the restart
// Unix only: the personal server replaces itself under systemd on Linux; the desktop on Windows never does.
#[cfg(unix)]
#[tokio::test]
async fn error_systemd_refuses_the_restart() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    *lock(&s.platform.restart) = Err("Access denied".to_owned());
    s.offer("0.5.0", archive("0.5.0", Hook::File), None, None)
        .await;
    assert_eq!(
        code(s.env.setup.run_server_update().await),
        "SETUP_UPDATE_REPLACE_FAILED"
    );
    assert_eq!(s.file("ordin-server"), "old server");
    assert_eq!(s.file("ordin-hook"), "old hook");
    assert_eq!(s.row().await.map(|r| r.1).as_deref(), Some("failed"));
}

// Scenario: Error — the archive holds a symbolic link
#[tokio::test]
async fn error_the_archive_holds_a_symbolic_link() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer("0.5.0", archive("0.5.0", Hook::Symlink), None, None)
        .await;
    assert_eq!(
        code(s.env.setup.run_server_update().await),
        "SETUP_UPDATE_FEED_INVALID"
    );
    assert!(s.leftovers().is_empty());
    assert_eq!(s.file("ordin-hook"), "old hook");
}

// Scenario: Error — a download address that is not https
#[tokio::test]
async fn error_a_download_address_that_is_not_https() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer(
        "0.5.0",
        archive("0.5.0", Hook::File),
        None,
        Some("http://downloads.example/ordin-server.tar.gz".to_owned()),
    )
    .await;
    assert_eq!(
        code(s.env.setup.run_server_update().await),
        "SETUP_UPDATE_FEED_INVALID"
    );
    let fetched = s
        .mock
        .received_requests()
        .await
        .expect("requests")
        .iter()
        .filter(|r| r.url.path().ends_with(".tar.gz"))
        .count();
    assert_eq!(fetched, 0);
}

// Scenario: Edge — a server never updates unasked
#[tokio::test]
async fn edge_a_server_never_updates_unasked() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer("0.5.0", archive("0.5.0", Hook::File), None, None)
        .await;
    assert_eq!(
        s.env
            .setup
            .check_update_feed()
            .await
            .expect("feed")
            .as_deref(),
        Some("0.5.0")
    );
    s.env
        .setup
        .run_checks(CheckTrigger::Background)
        .await
        .expect("checks");
    assert_eq!(s.file("ordin-server"), "old server");
    assert_eq!(s.platform.restarts.load(Ordering::SeqCst), 0);
}

// ---------------------------------------------------------------- ADR-072

// Scenario: Happy path — a whole-computer install is told to ask its administrator
#[tokio::test]
async fn happy_path_a_whole_computer_install_is_told_to_ask_its_administrator() {
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.kind) = InstallKind::WholeComputer;
    let env = desktop(updater.clone()).await;
    let offer = env
        .setup
        .check_update()
        .await
        .expect("check")
        .expect("offer");
    assert_eq!(offer.state, UpdateState::AdministratorNeeded);
    assert_eq!(updater.downloads.load(Ordering::SeqCst), 0);
    assert!(store::whats_new(&env.pool, "0.2.0")
        .await
        .expect("row")
        .is_some());
}

// Scenario: Error — a whole-computer install cannot install an update
#[tokio::test]
async fn error_a_whole_computer_install_cannot_install_an_update() {
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.kind) = InstallKind::WholeComputer;
    let env = desktop(updater).await;
    env.setup.check_update().await.expect("check");
    assert_eq!(code(env.setup.install_update().await), "SETUP_UPDATE_NONE");
}

// Scenario: Happy path — What's new shows after an administrator updates
#[tokio::test]
async fn happy_path_whats_new_shows_after_an_administrator_updates() {
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.kind) = InstallKind::WholeComputer;
    let env = desktop(updater).await;
    env.setup.check_update().await.expect("check");
    let Env { dir, store, .. } = env;
    let after = Env::on(
        dir,
        store,
        Options {
            version: "0.2.0",
            ..Options::default()
        },
    )
    .await;
    let notes = after.setup.whats_new().await.expect("read").expect("row");
    assert_eq!(notes.version, "0.2.0");
    assert!(!notes.seen);
}

// Scenario: Edge — two installs on one account
#[tokio::test]
async fn edge_two_installs_on_one_account() {
    let updater = FakeUpdater::offering("0.1.0");
    *lock(&updater.other) = Some(PathBuf::from("/Applications/ordin.app"));
    let env = desktop(updater).await;
    let report = env
        .setup
        .run_checks(CheckTrigger::Start)
        .await
        .expect("checks");
    let second = report
        .results
        .iter()
        .find(|r| r.check == CheckKind::SecondInstall)
        .expect("second install");
    assert_eq!(second.outcome, CheckOutcome::Failed);
    assert_eq!(second.code.as_deref(), Some("SETUP_SECOND_INSTALL"));
    assert_eq!(second.message_id.as_deref(), Some("setup-second-install"));
    assert_eq!(second.message_args, args(&[("folder", "/Applications")]));
    assert_eq!(second.action, None);
    let others_failed = report
        .results
        .iter()
        .filter(|r| r.check != CheckKind::SecondInstall)
        .any(|r| r.outcome == CheckOutcome::Failed);
    assert!(!others_failed);
}

// Scenario: Edge — one install on one account
#[tokio::test]
async fn edge_one_install_on_one_account() {
    let env = desktop(FakeUpdater::offering("0.1.0")).await;
    let report = env
        .setup
        .run_checks(CheckTrigger::Start)
        .await
        .expect("checks");
    let second = report
        .results
        .iter()
        .find(|r| r.check == CheckKind::SecondInstall)
        .expect("second install");
    assert_eq!(second.outcome, CheckOutcome::Passed);
}

// ---------------------------------------------------------------- SET-REC-2, the highest version started

// Scenario: Happy path — the first start records the running version (SET-REC-2)
#[tokio::test]
async fn happy_path_the_first_start_records_the_running_version() {
    let env = Env::with(Options {
        version: "0.2.0",
        ..Options::default()
    })
    .await;
    assert_eq!(env.setup.record_started().await.expect("recorded"), None);
    assert_eq!(
        store::highest_started(&env.pool).await.expect("read"),
        Some("0.2.0".to_owned())
    );
}

// Scenario: Happy path — a newer version replaces the record (SET-REC-2)
#[tokio::test]
async fn happy_path_a_newer_version_replaces_the_record() {
    let env = Env::with(Options {
        version: "0.3.0",
        ..Options::default()
    })
    .await;
    store::record_started(&env.pool, "0.2.0", ordin_types::time::Timestamp::now())
        .await
        .expect("seed");
    assert_eq!(
        env.setup.record_started().await.expect("recorded"),
        Some("0.2.0".to_owned())
    );
    assert_eq!(
        store::highest_started(&env.pool).await.expect("read"),
        Some("0.3.0".to_owned())
    );
}

// Scenario: Edge case — a lower version keeps the record and hides What's new (SET-REC-2)
#[tokio::test]
async fn edge_case_a_lower_version_keeps_the_record_and_hides_whats_new() {
    let env = Env::with(Options {
        version: "0.2.0",
        ..Options::default()
    })
    .await;
    store::record_started(&env.pool, "0.3.0", ordin_types::time::Timestamp::now())
        .await
        .expect("seed");
    store::keep_whats_new(
        &env.pool,
        "0.2.0",
        "Faster search",
        ordin_types::time::Timestamp::now(),
    )
    .await
    .expect("keep");
    assert_eq!(
        env.setup.record_started().await.expect("recorded"),
        Some("0.3.0".to_owned())
    );
    assert_eq!(
        store::highest_started(&env.pool).await.expect("read"),
        Some("0.3.0".to_owned())
    );
    assert_eq!(env.setup.whats_new().await.expect("read"), None);
}

// Edge case — an unreadable record is replaced by the running version (SET-REC-2.1)
#[tokio::test]
async fn edge_case_an_unreadable_record_is_replaced() {
    let env = Env::with(Options {
        version: "0.2.0",
        ..Options::default()
    })
    .await;
    store::record_started(
        &env.pool,
        "not-a-version",
        ordin_types::time::Timestamp::now(),
    )
    .await
    .expect("seed");
    assert_eq!(env.setup.record_started().await.expect("recorded"), None);
    assert_eq!(
        store::highest_started(&env.pool).await.expect("read"),
        Some("0.2.0".to_owned())
    );
}
