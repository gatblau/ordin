//! OrdinDesktop.md amendment 2026-01-28 (DSK-MD-1 to DSK-MD-12): one test per added Gherkin scenario
//! and per added Error Table row, against a stand-in host (`wiremock`) serving a test zip of ten small
//! files and `LICENSES.md`. No test reaches the network.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_memory_service::{ModelFileDigest, ModelManifest};
use ordin_service_api::commands::{MemoryModelsState, MemoryModelsView};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::*;
use crate::test_support::TempDir;

// ---- the test zip ----------------------------------------------------------------------------

/// Bytes that do not compress, so the zip is a few kilobytes.
fn noise(seed: u32, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x & 0xff) as u8
        })
        .collect()
}

struct TestZip {
    bytes: Vec<u8>,
    manifest: ModelManifest,
    sha256: String,
}

fn sha_hex(bytes: &[u8]) -> String {
    ordin_types::num::Sha256Hex::from_digest(&Sha256::digest(bytes).into()).0
}

/// Ten files, `LICENSES.md` and `SHA256SUMS`, as `scripts/release/memory-models.sh` packs them.
fn test_zip() -> TestZip {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut files = Vec::new();
    let mut sums = String::new();
    for (i, name) in MODEL_FILES.iter().enumerate() {
        let content = noise(i as u32 + 1, 600);
        writer.start_file(*name, options).expect("entry");
        writer.write_all(&content).expect("content");
        let digest = sha_hex(&content);
        sums.push_str(&format!("{digest}  {name}\n"));
        files.push(ModelFileDigest {
            path: (*name).to_owned(),
            sha256: digest,
        });
    }
    writer.start_file(LICENCES, options).expect("entry");
    writer.write_all(b"MIT and Apache-2.0\n").expect("licences");
    writer.start_file("SHA256SUMS", options).expect("entry");
    writer.write_all(sums.as_bytes()).expect("sums");
    let bytes = writer.finish().expect("zip").into_inner();
    TestZip {
        sha256: sha_hex(&bytes),
        bytes,
        manifest: ModelManifest { files },
    }
}

// ---- the stand-in host -----------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Mode {
    /// `200` for the whole zip, `206` from a range.
    Whole,
    /// The first request (no range) gets `206` with only the first `n` bytes; then as `Whole`.
    FirstCut(usize),
    /// Every body ends `n` bytes early; every answer is `206`.
    ShortEveryTime(usize),
    /// `200` announcing and carrying one byte more than the zip.
    Larger,
    Status(u16),
    /// As `Whole`, after a delay.
    Slow(Duration),
}

struct Host {
    zip: Vec<u8>,
    mode: Mode,
    seen: AtomicUsize,
}

fn range_start(request: &Request) -> Option<usize> {
    let value = request.headers.get("range")?.to_str().ok()?;
    value
        .strip_prefix("bytes=")?
        .strip_suffix('-')?
        .parse()
        .ok()
}

fn partial(zip: &[u8], start: usize, end: usize) -> ResponseTemplate {
    let end = end.max(start);
    ResponseTemplate::new(206)
        .insert_header(
            "content-range",
            format!("bytes {start}-{}/{}", zip.len() - 1, zip.len()).as_str(),
        )
        .set_body_bytes(zip[start..end].to_vec())
}

impl Respond for Host {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let first = self.seen.fetch_add(1, Ordering::SeqCst) == 0;
        let len = self.zip.len();
        let start = range_start(request);
        match self.mode {
            Mode::Status(status) => ResponseTemplate::new(status),
            Mode::Larger => {
                let mut body = self.zip.clone();
                body.push(0);
                ResponseTemplate::new(200).set_body_bytes(body)
            }
            Mode::ShortEveryTime(n) => partial(&self.zip, start.unwrap_or(0), len - n),
            Mode::FirstCut(n) if first && start.is_none() => partial(&self.zip, 0, n),
            Mode::Slow(delay) => whole(&self.zip, start).set_delay(delay),
            Mode::Whole | Mode::FirstCut(_) => whole(&self.zip, start),
        }
    }
}

fn whole(zip: &[u8], start: Option<usize>) -> ResponseTemplate {
    match start {
        Some(start) => partial(zip, start, zip.len()),
        None => ResponseTemplate::new(200).set_body_bytes(zip.to_vec()),
    }
}

async fn host(zip: &TestZip, mode: Mode) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models-1/ordin-memory-models-1.zip"))
        .respond_with(Host {
            zip: zip.bytes.clone(),
            mode,
            seen: AtomicUsize::new(0),
        })
        .mount(&server)
        .await;
    server
}

fn source_at(base: &str, zip: &TestZip) -> ModelsSource {
    ModelsSource {
        url: reqwest::Url::parse(&format!("{base}/models-1/ordin-memory-models-1.zip"))
            .expect("url"),
        sha256: zip.sha256.clone(),
        bytes: zip.bytes.len() as u64,
    }
}

async fn ranges_sent(server: &MockServer) -> Vec<Option<usize>> {
    server
        .received_requests()
        .await
        .expect("recorded")
        .iter()
        .map(range_start)
        .collect()
}

// ---- the Memory Service stand-in and the events --------------------------------------------

/// `recheck_models` checks the files' digests as the Memory Service does (MEM-MD-3).
struct FileGate {
    dir: PathBuf,
    manifest: ModelManifest,
    ready: AtomicBool,
    rechecks: AtomicUsize,
    wanted: watch::Sender<u64>,
}

impl FileGate {
    fn new(dir: &Path, manifest: &ModelManifest, ready: bool) -> Arc<FileGate> {
        Arc::new(FileGate {
            dir: dir.to_path_buf(),
            manifest: manifest.clone(),
            ready: AtomicBool::new(ready),
            rechecks: AtomicUsize::new(0),
            wanted: watch::Sender::new(0),
        })
    }

    /// A recall or remember refused for a missing file.
    fn refuse_once(&self) {
        self.wanted.send_modify(|n| *n += 1);
    }
}

#[async_trait::async_trait]
impl ModelsGate for FileGate {
    async fn recheck_models(&self) -> bool {
        self.rechecks.fetch_add(1, Ordering::SeqCst);
        let ok = self
            .manifest
            .files
            .iter()
            .all(|f| sha256_of(&self.dir.join(&f.path)).is_ok_and(|digest| digest == f.sha256));
        if ok {
            self.ready.store(true, Ordering::SeqCst);
        }
        ok
    }
    fn models_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
    fn models_wanted(&self) -> watch::Receiver<u64> {
        self.wanted.subscribe()
    }
}

#[derive(Default)]
struct Recorded(Mutex<Vec<MemoryModelsView>>);

impl ModelsEvents for Recorded {
    fn changed(&self, view: &MemoryModelsView) {
        self.0.lock().expect("events").push(view.clone());
    }
}

impl Recorded {
    fn all(&self) -> Vec<MemoryModelsView> {
        self.0.lock().expect("events").clone()
    }
}

fn rules() -> DownloadRules {
    DownloadRules {
        status_timeout: Duration::from_secs(5),
        chunk_timeout: Duration::from_secs(5),
        backoff_unit: Duration::from_millis(10),
        backoff_cap: Duration::from_millis(200),
        progress_every: Duration::ZERO,
        ask_again_after: Duration::from_millis(400),
        ..DownloadRules::default()
    }
}

struct Rig {
    _dir: TempDir,
    models_dir: PathBuf,
    work_dir: PathBuf,
    gate: Arc<FileGate>,
    events: Arc<Recorded>,
    models: MemoryModels,
    shutdown: CancellationToken,
}

impl Rig {
    fn part(&self, zip: &TestZip) -> PathBuf {
        self.work_dir.join(format!("{}.zip.part", zip.sha256))
    }
}

struct Setup {
    source: Option<ModelsSource>,
    manifest: ModelManifest,
    rules: DownloadRules,
    ready: bool,
    /// Called with the data root before start.
    before: Box<dyn FnOnce(&Path)>,
}

impl Setup {
    fn new(source: Option<ModelsSource>, zip: &TestZip) -> Setup {
        Setup {
            source,
            manifest: zip.manifest.clone(),
            rules: rules(),
            ready: false,
            before: Box::new(|_| {}),
        }
    }
}

fn rig(setup: Setup) -> Rig {
    let dir = TempDir::new("models");
    let models_dir = dir.path().join("models");
    let work_dir = dir.path().join("models.download");
    (setup.before)(dir.path());
    let gate = FileGate::new(&models_dir, &setup.manifest, setup.ready);
    let events = Arc::new(Recorded::default());
    let shutdown = CancellationToken::new();
    let models = MemoryModels::start(ModelsDeps {
        source: setup.source,
        manifest: setup.manifest,
        models_dir: models_dir.clone(),
        work_dir: work_dir.clone(),
        http: crate::http::outbound_client().expect("client"),
        gate: gate.clone(),
        events: events.clone(),
        shutdown: shutdown.clone(),
        rules: setup.rules,
    });
    Rig {
        _dir: dir,
        models_dir,
        work_dir,
        gate,
        events,
        models,
        shutdown,
    }
}

/// Waits up to 20 s for the view to satisfy `done`.
async fn until(
    models: &MemoryModels,
    done: impl Fn(&MemoryModelsView) -> bool,
) -> MemoryModelsView {
    for _ in 0..2000 {
        let view = models.view();
        if done(&view) {
            return view;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the view never got there: {:?}", models.view());
}

async fn settled(models: &MemoryModels) -> MemoryModelsView {
    until(models, |v| {
        matches!(
            v.state,
            MemoryModelsState::Ready | MemoryModelsState::Failed
        )
    })
    .await
}

fn failure(view: &MemoryModelsView) -> Option<&str> {
    view.failure.as_deref()
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// `<data root>/models` as an earlier run left it, with a marker file.
fn earlier_models(root: &Path) {
    let dir = root.join("models");
    std::fs::create_dir_all(&dir).expect("models");
    std::fs::write(dir.join("marker"), b"earlier").expect("marker");
}

// ---- Gherkin: OrdinDesktop.md, Feature: The memory models download on first use -------------

// Scenario: Happy path — the models download, are checked and memory works (DSK-MD-6, DSK-MD-9, DSK-MD-10)
#[tokio::test]
async fn happy_path_the_models_download_are_checked_and_memory_works() {
    let zip = test_zip();
    let server = host(&zip, Mode::Whole).await;
    let rig = rig(Setup::new(Some(source_at(&server.uri(), &zip)), &zip));
    assert_eq!(rig.models.view().state, MemoryModelsState::Missing);
    rig.gate.refuse_once();
    let asked = until(&rig.models, |v| v.ask).await;
    assert_eq!(asked.state, MemoryModelsState::Missing);
    assert_eq!(asked.host, "127.0.0.1");

    let answer = rig.models.download().expect("started");
    assert_eq!(answer.state, MemoryModelsState::Downloading);
    assert!(!answer.ask);
    let done = settled(&rig.models).await;
    assert_eq!(done.state, MemoryModelsState::Ready, "{done:?}");
    assert_eq!(done.received_bytes, zip.bytes.len() as u64);

    let events = rig.events.all();
    let states: Vec<MemoryModelsState> = events.iter().map(|v| v.state).collect();
    let at = |s| {
        states
            .iter()
            .position(|x| *x == s)
            .expect("the state was emitted")
    };
    assert!(at(MemoryModelsState::Downloading) < at(MemoryModelsState::Checking));
    assert!(at(MemoryModelsState::Checking) < at(MemoryModelsState::Ready));
    let received: Vec<u64> = events
        .iter()
        .filter(|v| v.state == MemoryModelsState::Downloading)
        .map(|v| v.received_bytes)
        .collect();
    assert!(received.windows(2).all(|w| w[0] <= w[1]), "{received:?}");
    assert!(events.iter().all(|v| v.received_bytes <= v.size_bytes));
    assert_eq!(received.last().copied(), Some(zip.bytes.len() as u64));

    let mut expected: Vec<String> = vec![
        "LICENSES.md".to_owned(),
        "embedding".to_owned(),
        "reranker".to_owned(),
    ];
    expected.sort();
    assert_eq!(names_in(&rig.models_dir), expected);
    for file in MODEL_FILES {
        assert!(rig.models_dir.join(file).is_file(), "{file}");
    }
    assert!(!rig.models_dir.join("SHA256SUMS").exists());
    assert!(names_in(&rig.work_dir)
        .iter()
        .all(|n| !n.ends_with(".zip.part")));
    assert_eq!(rig.gate.rechecks.load(Ordering::SeqCst), 1);
    assert!(rig.gate.models_ready());
    rig.shutdown.cancel();
}

// Scenario: Edge case — a second start downloads nothing (DSK-MD-4)
#[tokio::test]
async fn edge_case_a_second_start_downloads_nothing() {
    let zip = test_zip();
    let server = host(&zip, Mode::Whole).await;
    let mut setup = Setup::new(Some(source_at(&server.uri(), &zip)), &zip);
    setup.ready = true;
    let rig = rig(setup);
    let view = rig.models.view();
    assert_eq!(view.state, MemoryModelsState::Ready);
    assert_eq!(view.received_bytes, view.size_bytes);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(server
        .received_requests()
        .await
        .expect("recorded")
        .is_empty());
    // Download in `ready` answers the view and starts nothing.
    assert_eq!(
        rig.models.download().expect("view").state,
        MemoryModelsState::Ready
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(server
        .received_requests()
        .await
        .expect("recorded")
        .is_empty());
    rig.shutdown.cancel();
}

// Scenario: Edge case — interrupted, then resumed by byte range (DSK-MD-6, DSK-MD-7)
#[tokio::test]
async fn edge_case_interrupted_then_resumed_by_byte_range() {
    let zip = test_zip();
    let cut = zip.bytes.len() * 2 / 5;
    let server = host(&zip, Mode::FirstCut(cut)).await;
    let rig = rig(Setup::new(Some(source_at(&server.uri(), &zip)), &zip));
    rig.models.download().expect("started");
    let done = settled(&rig.models).await;
    assert_eq!(done.state, MemoryModelsState::Ready, "{done:?}");
    assert_eq!(ranges_sent(&server).await, vec![None, Some(cut)]);
    rig.shutdown.cancel();
}

// Scenario: Edge case — stopped by the person, resumed after a restart (DSK-MD-11)
#[tokio::test]
async fn edge_case_stopped_by_the_person_resumed_after_a_restart() {
    let zip = test_zip();
    // Stop while downloading: `missing`, `ask` false, nothing more happens.
    let slow = host(&zip, Mode::Slow(Duration::from_secs(10))).await;
    let rig1 = rig(Setup::new(Some(source_at(&slow.uri(), &zip)), &zip));
    rig1.models.download().expect("started");
    until(&rig1.models, |v| v.state == MemoryModelsState::Downloading).await;
    let stopped = rig1.models.stop();
    assert_eq!(stopped.state, MemoryModelsState::Missing);
    assert!(!stopped.ask);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(rig1.models.view().state, MemoryModelsState::Missing);
    // Stop in any other state does nothing.
    assert_eq!(rig1.models.stop().state, MemoryModelsState::Missing);
    rig1.shutdown.cancel();

    // After a restart the partial file is resumed from its length.
    let server = host(&zip, Mode::Whole).await;
    let kept = zip.bytes.len() / 2;
    let head = zip.bytes[..kept].to_vec();
    let sha = zip.sha256.clone();
    let mut setup = Setup::new(Some(source_at(&server.uri(), &zip)), &zip);
    setup.before = Box::new(move |root| {
        let work = root.join("models.download");
        std::fs::create_dir_all(&work).expect("work");
        std::fs::write(work.join(format!("{sha}.zip.part")), head).expect("part");
    });
    let rig2 = rig(setup);
    let view = rig2.models.view();
    assert_eq!(view.state, MemoryModelsState::Missing);
    assert_eq!(view.received_bytes, kept as u64);
    rig2.models.download().expect("started");
    assert_eq!(settled(&rig2.models).await.state, MemoryModelsState::Ready);
    assert_eq!(ranges_sent(&server).await, vec![Some(kept)]);
    rig2.shutdown.cancel();
}

// Scenario: Error path — a short file every time (DSK-MD-7)
#[tokio::test]
async fn error_path_a_short_file_every_time() {
    let zip = test_zip();
    let server = host(&zip, Mode::ShortEveryTime(1_000)).await;
    let rig = rig(Setup::new(Some(source_at(&server.uri(), &zip)), &zip));
    rig.models.download().expect("started");
    let done = settled(&rig.models).await;
    assert_eq!(failure(&done), Some(MODELS_INTERRUPTED), "{done:?}");
    assert_eq!(server.received_requests().await.expect("recorded").len(), 6);
    let kept = (zip.bytes.len() - 1_000) as u64;
    assert_eq!(file_len(&rig.part(&zip)), kept, "the partial file is kept");
    assert_eq!(done.received_bytes, kept);
    rig.shutdown.cancel();
}

// The waits of DSK-MD-7: before attempt k (k ≥ 2), at most min(30 s, 1 s × 2^(k−2)).
#[test]
fn each_wait_is_no_longer_than_the_backoff_cap() {
    let rules = DownloadRules::default();
    let caps: Vec<u64> = (2..=8).map(|k| backoff_cap(&rules, k).as_secs()).collect();
    assert_eq!(caps, vec![1, 2, 4, 8, 16, 30, 30]);
    assert_eq!(backoff_cap(&rules, 40), Duration::from_secs(30));
    assert_eq!(rules.attempts, 6);
    assert_eq!(rules.status_timeout, Duration::from_secs(30));
    assert_eq!(rules.chunk_timeout, Duration::from_secs(30));
    assert_eq!(rules.reset_after, 1024 * 1024);
}

// Scenario: Error path — the zip's fingerprint is wrong (DSK-MD-9)
#[tokio::test]
async fn error_path_the_zips_fingerprint_is_wrong() {
    let zip = test_zip();
    let server = host(&zip, Mode::Whole).await;
    let mut source = source_at(&server.uri(), &zip);
    source.sha256 = "0".repeat(64);
    let part = rig_part_name(&source);
    let mut setup = Setup::new(Some(source), &zip);
    setup.before = Box::new(earlier_models);
    let rig = rig(setup);
    rig.models.download().expect("started");
    let done = settled(&rig.models).await;
    assert_eq!(failure(&done), Some(MODELS_MISMATCH));
    assert!(
        !rig.work_dir.join(part).exists(),
        "the partial file is deleted"
    );
    assert!(names_in(&rig.work_dir)
        .iter()
        .all(|n| !n.starts_with("unpack-")));
    assert_eq!(names_in(&rig.models_dir), vec!["marker".to_owned()]);
    assert_eq!(rig.gate.rechecks.load(Ordering::SeqCst), 0);
    rig.shutdown.cancel();
}

fn rig_part_name(source: &ModelsSource) -> String {
    format!("{}.zip.part", source.sha256)
}

// Scenario: Error path — one file's fingerprint is wrong inside a matching zip (DSK-MD-9)
#[tokio::test]
async fn error_path_one_files_fingerprint_is_wrong_inside_a_matching_zip() {
    let zip = test_zip();
    let server = host(&zip, Mode::Whole).await;
    let mut setup = Setup::new(Some(source_at(&server.uri(), &zip)), &zip);
    for file in &mut setup.manifest.files {
        if file.path == "reranker/tokenizer.json" {
            file.sha256 = "1".repeat(64);
        }
    }
    setup.before = Box::new(earlier_models);
    let rig = rig(setup);
    rig.models.download().expect("started");
    let done = settled(&rig.models).await;
    assert_eq!(failure(&done), Some(MODELS_MISMATCH));
    assert!(names_in(&rig.work_dir)
        .iter()
        .all(|n| !n.starts_with("unpack-")));
    assert_eq!(names_in(&rig.models_dir), vec!["marker".to_owned()]);
    rig.shutdown.cancel();
}

// Scenario: Error path — the host announces a larger file (DSK-MD-8)
#[tokio::test]
async fn error_path_the_host_announces_a_larger_file() {
    let zip = test_zip();
    let server = host(&zip, Mode::Larger).await;
    let rig = rig(Setup::new(Some(source_at(&server.uri(), &zip)), &zip));
    rig.models.download().expect("started");
    let done = settled(&rig.models).await;
    assert_eq!(failure(&done), Some(MODELS_MISMATCH));
    assert_eq!(
        file_len(&rig.part(&zip)),
        0,
        "no byte is kept past size_bytes"
    );
    assert_eq!(server.received_requests().await.expect("recorded").len(), 1);
    rig.shutdown.cancel();
}

// Scenario: Error path — the disk is full (DSK-MD-8)
#[tokio::test]
async fn error_path_the_disk_is_full() {
    let zip = test_zip();
    let server = host(&zip, Mode::Whole).await;
    let mut setup = Setup::new(Some(source_at(&server.uri(), &zip)), &zip);
    setup.rules.storage_full_after = Some(1_000);
    let rig = rig(setup);
    rig.models.download().expect("started");
    let done = settled(&rig.models).await;
    assert_eq!(failure(&done), Some(MODELS_DISK_FULL));
    assert_eq!(file_len(&rig.part(&zip)), 1_000, "what was written is kept");
    assert_eq!(done.received_bytes, 1_000);
    rig.shutdown.cancel();
}

// Scenario: Error path — offline (DSK-MD-7)
#[tokio::test]
async fn error_path_offline() {
    let zip = test_zip();
    // An address that refuses every connection: a port bound, then released.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    drop(listener);
    let rig = rig(Setup::new(
        Some(source_at(&format!("http://127.0.0.1:{port}"), &zip)),
        &zip,
    ));
    rig.models.download().expect("started");
    let done = settled(&rig.models).await;
    assert_eq!(failure(&done), Some(MODELS_OFFLINE), "{done:?}");
    assert_eq!(done.received_bytes, 0);
    rig.shutdown.cancel();
}

// Scenario: Error path — the address is blocked (DSK-MD-6)
#[tokio::test]
async fn error_path_the_address_is_blocked() {
    let zip = test_zip();
    for status in [401u16, 403, 404, 407, 410, 451, 302] {
        let server = host(&zip, Mode::Status(status)).await;
        let rig = rig(Setup::new(Some(source_at(&server.uri(), &zip)), &zip));
        rig.models.download().expect("started");
        let done = settled(&rig.models).await;
        assert_eq!(failure(&done), Some(MODELS_BLOCKED), "{status}");
        assert_eq!(
            server.received_requests().await.expect("recorded").len(),
            1,
            "{status}: one attempt"
        );
        rig.shutdown.cancel();
    }
}

// DSK-MD-6: `408`, `429` and any `5xx` are retried, then `DSK_MODELS_INTERRUPTED` with no byte
// received (the last failure was not a connection failure).
#[tokio::test]
async fn a_busy_host_is_retried_six_times() {
    let zip = test_zip();
    let server = host(&zip, Mode::Status(503)).await;
    let rig = rig(Setup::new(Some(source_at(&server.uri(), &zip)), &zip));
    rig.models.download().expect("started");
    let done = settled(&rig.models).await;
    assert_eq!(failure(&done), Some(MODELS_INTERRUPTED));
    assert_eq!(server.received_requests().await.expect("recorded").len(), 6);
    rig.shutdown.cancel();
}

// Scenario: Edge case — Not now, then asked again at the next use (DSK-MD-5)
#[tokio::test]
async fn edge_case_not_now_then_asked_again_at_the_next_use() {
    let zip = test_zip();
    let server = host(&zip, Mode::Whole).await;
    let rig = rig(Setup::new(Some(source_at(&server.uri(), &zip)), &zip));
    rig.gate.refuse_once();
    until(&rig.models, |v| v.ask).await;
    let declined = rig.models.decline();
    assert!(!declined.ask);
    assert_eq!(declined.state, MemoryModelsState::Missing);
    // A refusal soon after (the spec's 10 seconds, within the 60) does not ask again.
    let before = rig.events.all().len();
    rig.gate.refuse_once();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!rig.models.view().ask);
    assert_eq!(rig.events.all().len(), before);
    // A refusal after the interval (the spec's 2 minutes) asks again and emits.
    tokio::time::sleep(Duration::from_millis(450)).await;
    rig.gate.refuse_once();
    until(&rig.models, |v| v.ask).await;
    assert!(rig.events.all().last().is_some_and(|v| v.ask));
    assert!(
        server
            .received_requests()
            .await
            .expect("recorded")
            .is_empty(),
        "nothing downloaded"
    );
    rig.shutdown.cancel();
}

// Scenario: Happy path — missing models are offered at start (DSK-MD-15)
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-15
#[tokio::test]
async fn happy_path_missing_models_are_offered_at_start() {
    let zip = test_zip();
    let server = host(&zip, Mode::Whole).await;
    let rig = rig(Setup::new(Some(source_at(&server.uri(), &zip)), &zip));
    let view = rig.models.view();
    assert_eq!(view.state, MemoryModelsState::Missing);
    assert!(view.ask, "asked at start, before memory is used");
    // Not now holds for this run; a refusal within the interval does not ask again.
    assert!(!rig.models.decline().ask);
    rig.gate.refuse_once();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!rig.models.view().ask);
    assert!(
        server
            .received_requests()
            .await
            .expect("recorded")
            .is_empty(),
        "nothing downloaded until the person agrees"
    );
    rig.shutdown.cancel();
}

// Scenario: Edge case — a build with no models input (DSK-MD-2)
#[tokio::test]
async fn edge_case_a_build_with_no_models_input() {
    let zip = test_zip();
    let rig = rig(Setup::new(None, &zip));
    let view = rig.models.view();
    assert_eq!(view.state, MemoryModelsState::NotInBuild);
    assert_eq!((view.size_bytes, view.host.as_str()), (0, ""));
    assert_eq!(rig.models.download(), Err(MODELS_NOT_IN_BUILD));
    // A refused recall does not ask in a build with no models input.
    rig.gate.refuse_once();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!rig.models.view().ask);

    // Through the Service API (SVC-MD-3; Error Table row "in a build with no models input").
    let api = crate::service_api::DesktopServiceApi::new(
        crate::service_api::ApiModules {
            memory_models: Some(rig.models.clone()),
            ..Default::default()
        },
        Arc::new(Sentences),
    );
    let ctx = ordin_service_api::CallContext {
        request_id: ordin_types::CommandRequestId::new(),
        correlation_id: "test".to_owned(),
        origin: ordin_service_api::CallOrigin::LocalScreens,
        deadline: std::time::Instant::now() + Duration::from_secs(5),
        cancel: CancellationToken::new(),
    };
    let call = |name: &'static str| {
        let api = &api;
        let ctx = &ctx;
        async move {
            ordin_service_api::dispatch(
                api,
                ctx,
                ordin_service_api::command(name).expect("row"),
                serde_json::json!({}),
            )
            .await
        }
    };
    let refused = call("app.download_memory_models")
        .await
        .expect_err("refused");
    assert_eq!(refused.code, MODELS_NOT_IN_BUILD);
    assert_eq!(refused.action, None, "not retryable");
    let status = call("app.memory_models").await.expect("the view");
    assert_eq!(status["state"], "not_in_build");
    assert!(status.get("failure").is_none());
    assert_eq!(
        call("app.stop_memory_models_download").await.expect("view")["state"],
        "not_in_build"
    );
    assert_eq!(
        call("app.decline_memory_models").await.expect("view")["ask"],
        false
    );
    rig.shutdown.cancel();
}

struct Sentences;

impl ordin_service_api::ErrorSentences for Sentences {
    fn sentence(&self, code: &str) -> String {
        code.to_owned()
    }
    fn action(&self, _code: &str) -> Option<ordin_types::RepairAction> {
        None
    }
}

// DSK-MD-4: the clean-up at start keeps only this zip's partial file.
#[tokio::test]
async fn at_start_the_working_folder_is_cleaned_and_this_zips_partial_file_kept() {
    let zip = test_zip();
    let server = host(&zip, Mode::Whole).await;
    let sha = zip.sha256.clone();
    let mut setup = Setup::new(Some(source_at(&server.uri(), &zip)), &zip);
    setup.before = Box::new(move |root| {
        let work = root.join("models.download");
        std::fs::create_dir_all(work.join("unpack-0192")).expect("unpack");
        std::fs::create_dir_all(work.join("old-0192")).expect("old");
        std::fs::write(work.join(format!("{}.zip.part", "a".repeat(64))), b"other").expect("other");
        std::fs::write(work.join(format!("{sha}.zip.part")), b"mine").expect("mine");
    });
    let rig = rig(setup);
    assert_eq!(rig.models.view().received_bytes, 4);
    for _ in 0..200 {
        if names_in(&rig.work_dir).len() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        names_in(&rig.work_dir),
        vec![format!("{}.zip.part", zip.sha256)]
    );
    assert!(server
        .received_requests()
        .await
        .expect("recorded")
        .is_empty());
    rig.shutdown.cancel();
}

// DSK-MD-2: the four build-time variables.
#[test]
fn the_models_input_is_valid_only_as_the_spec_states() {
    let zip = test_zip();
    let manifest: String = zip
        .manifest
        .files
        .iter()
        .map(|f| format!("{}={}", f.path, f.sha256))
        .collect::<Vec<_>>()
        .join(",");
    let url = "https://github.com/gatblau/ordin-releases/releases/download/models-1/ordin-memory-models-1.zip";
    let sha = "c97ba4951a33720e317d1b95f5859f0fba075e2c0b5fa4e19bdb87694ac995d7";
    let good = models_input(Some(url), Some(sha), Some("173982577"), Some(&manifest));
    let source = good.source.expect("a models input");
    assert_eq!(source.bytes, 173_982_577);
    assert_eq!(host_of(&source.url), "github.com");
    assert_eq!(good.manifest, zip.manifest);

    assert_eq!(models_input(None, None, None, None), ModelsInput::default());
    let nine = manifest.rsplit_once(',').expect("pairs").0.to_owned();
    let doubled = format!("{nine},{}", manifest.split(',').next().expect("first"));
    let outside = manifest.replacen("embedding/config.json", "embedding/other.json", 1);
    let upper = manifest.replacen(
        &zip.manifest.files[0].sha256,
        &zip.manifest.files[0].sha256.to_uppercase(),
        1,
    );
    for (url, sha, bytes, manifest) in [
        (None, Some(sha), Some("1"), Some(manifest.as_str())),
        (
            Some("http://example.test/m.zip"),
            Some(sha),
            Some("1"),
            Some(manifest.as_str()),
        ),
        (
            Some(url),
            Some(&sha[1..]),
            Some("1"),
            Some(manifest.as_str()),
        ),
        (Some(url), Some(sha), Some("0"), Some(manifest.as_str())),
        (Some(url), Some(sha), Some("1,000"), Some(manifest.as_str())),
        (Some(url), Some(sha), Some("1"), None),
        (Some(url), Some(sha), Some("1"), Some(nine.as_str())),
        (Some(url), Some(sha), Some("1"), Some(doubled.as_str())),
        (Some(url), Some(sha), Some("1"), Some(outside.as_str())),
        (Some(url), Some(sha), Some("1"), Some(upper.as_str())),
    ] {
        assert_eq!(
            models_input(url, sha, bytes, manifest),
            ModelsInput::default(),
            "{url:?} {bytes:?}"
        );
    }
}

#[test]
fn a_content_range_is_read_as_its_start_and_total() {
    assert_eq!(content_range("bytes 40-99/100"), Some((40, 100)));
    assert_eq!(content_range("bytes */100"), None);
    assert_eq!(content_range("items 0-1/2"), None);
}
