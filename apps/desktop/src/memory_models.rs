//! Memory's models, fetched the first time memory is used (OrdinDesktop.md amendment 2026-01-28,
//! DSK-MD-1 to DSK-MD-12): the models input compiled into a release build, the state the four `app.*`
//! commands answer, the person's question, the download with its resume, retries and size cap, the
//! two fingerprint checks, the move into place and the Memory Service's re-check.
//!
//! Nothing here reaches the network unless the person pressed **Download** in a build with a models
//! input; a development or test build has none (DSK-MD-2).
//
// A zip that matched its fingerprint but cannot be unpacked is `DSK_MODELS_MISMATCH` (DSK-MD-13); a
// refused certificate, told apart by the word "certificate" in the error's chain of causes, is
// `DSK_MODELS_BLOCKED` (DSK-MD-14).
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-13
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-14

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ordin_memory_service::{MemoryService, ModelFileDigest, ModelManifest};
use ordin_service_api::commands::{MemoryModelsState, MemoryModelsView};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::error::{
    MODELS_BLOCKED, MODELS_DISK_FULL, MODELS_INTERRUPTED, MODELS_MISMATCH, MODELS_NOT_IN_BUILD,
    MODELS_OFFLINE, MODELS_WRITE_FAILED,
};

/// The zip a release build names (PB-REL-11). Program-internal; not a Service API type.
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-2
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelsSource {
    /// `https` only.
    pub url: reqwest::Url,
    /// 64 lowercase hexadecimal characters.
    pub sha256: String,
    /// The zip's exact size; also the download's size cap (DSK-MD-8).
    pub bytes: u64,
}

/// The ten files the Memory Service checks (PB-REL-12, MemoryService.md §1 step 3).
pub const MODEL_FILES: [&str; 10] = [
    "embedding/model.onnx",
    "embedding/tokenizer.json",
    "embedding/config.json",
    "embedding/special_tokens_map.json",
    "embedding/tokenizer_config.json",
    "reranker/model.onnx",
    "reranker/tokenizer.json",
    "reranker/config.json",
    "reranker/special_tokens_map.json",
    "reranker/tokenizer_config.json",
];

/// The licences entry unpacked beside the models (DSK-MD-1, DSK-MD-9).
const LICENCES: &str = "LICENSES.md";

/// The models input of DSK-MD-2: `CoreInputs.model_manifest` and `CoreInputs.models_source`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelsInput {
    pub manifest: ModelManifest,
    pub source: Option<ModelsSource>,
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The four build-time variables read as DSK-MD-2 states. Any variable absent, or the input invalid,
/// gives no models input; an invalid input is logged once, naming the first failing variable and
/// never its value. With none of the four set (a development or test build) nothing is logged.
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-2
pub fn models_input(
    url: Option<&str>,
    sha256: Option<&str>,
    bytes: Option<&str>,
    manifest: Option<&str>,
) -> ModelsInput {
    if url.is_none() && sha256.is_none() && bytes.is_none() && manifest.is_none() {
        return ModelsInput::default();
    }
    match parse_input(url, sha256, bytes, manifest) {
        Ok(input) => input,
        Err(variable) => {
            tracing::warn!(
                component = "desktop",
                variable,
                "memory_models.input_invalid"
            );
            ModelsInput::default()
        }
    }
}

fn parse_input(
    url: Option<&str>,
    sha256: Option<&str>,
    bytes: Option<&str>,
    manifest: Option<&str>,
) -> Result<ModelsInput, &'static str> {
    const URL: &str = "ORDIN_MEMORY_MODELS_URL";
    const SHA: &str = "ORDIN_MEMORY_MODELS_SHA256";
    const BYTES: &str = "ORDIN_MEMORY_MODELS_BYTES";
    const MANIFEST: &str = "ORDIN_MEMORY_MODELS_MANIFEST";
    let url = url
        .filter(|u| u.starts_with("https://"))
        .and_then(|u| reqwest::Url::parse(u).ok())
        .filter(|u| u.host_str().is_some_and(|h| !h.is_empty()))
        .ok_or(URL)?;
    let sha256 = sha256.filter(|s| is_sha256(s)).ok_or(SHA)?.to_owned();
    let bytes = bytes
        .filter(|b| !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()))
        .and_then(|b| b.parse::<u64>().ok())
        .filter(|b| *b > 0)
        .ok_or(BYTES)?;
    let manifest = parse_manifest(manifest.ok_or(MANIFEST)?).ok_or(MANIFEST)?;
    Ok(ModelsInput {
        manifest,
        source: Some(ModelsSource { url, sha256, bytes }),
    })
}

/// Exactly ten `<path>=<sha256>` pairs separated by `,`, whose paths are [`MODEL_FILES`], each once.
fn parse_manifest(text: &str) -> Option<ModelManifest> {
    let mut files = Vec::new();
    for pair in text.split(',') {
        let (path, sha256) = pair.split_once('=')?;
        if !MODEL_FILES.contains(&path) || !is_sha256(sha256) {
            return None;
        }
        if files.iter().any(|f: &ModelFileDigest| f.path == path) {
            return None;
        }
        files.push(ModelFileDigest {
            path: path.to_owned(),
            sha256: sha256.to_owned(),
        });
    }
    (files.len() == MODEL_FILES.len()).then_some(ModelManifest { files })
}

/// What the Memory Service offers the download (MemoryService.md MEM-MD-3).
#[async_trait::async_trait]
pub trait ModelsGate: Send + Sync {
    async fn recheck_models(&self) -> bool;
    fn models_ready(&self) -> bool;
    fn models_wanted(&self) -> watch::Receiver<u64>;
}

#[async_trait::async_trait]
impl ModelsGate for MemoryService {
    async fn recheck_models(&self) -> bool {
        MemoryService::recheck_models(self).await
    }
    fn models_ready(&self) -> bool {
        MemoryService::models_ready(self)
    }
    fn models_wanted(&self) -> watch::Receiver<u64> {
        MemoryService::models_wanted(self)
    }
}

/// Where `memory_models_changed` goes (SVC-MD-2). Called in the order of the changes.
pub trait ModelsEvents: Send + Sync {
    fn changed(&self, view: &MemoryModelsView);
}

/// The local backend's events: `memory_models_changed { models }` through `Journal::append_event`, as
/// the program emits `setup_progress`, in order, from one task.
pub struct JournalModelsEvents {
    sender: tokio::sync::mpsc::UnboundedSender<MemoryModelsView>,
}

impl JournalModelsEvents {
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-12
    pub fn new(
        journal: Arc<ordin_journal::Journal>,
        shutdown: CancellationToken,
    ) -> JournalModelsEvents {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<MemoryModelsView>();
        tokio::spawn(async move {
            loop {
                let view = tokio::select! {
                    _ = shutdown.cancelled() => return,
                    next = receiver.recv() => match next {
                        Some(view) => view,
                        None => return,
                    },
                };
                let data = serde_json::json!({ "models": view });
                if let Err(e) = journal.append_event("memory_models_changed", data).await {
                    use ordin_types::CodedError as _;
                    tracing::warn!(
                        component = "desktop",
                        code = e.code(),
                        "memory_models.event_failed"
                    );
                }
            }
        });
        JournalModelsEvents { sender }
    }
}

impl ModelsEvents for JournalModelsEvents {
    fn changed(&self, view: &MemoryModelsView) {
        let _ = self.sender.send(view.clone());
    }
}

/// The numbers of DSK-MD-5, DSK-MD-6 and DSK-MD-7. [`Default`] holds the spec's values; a test
/// passes smaller times.
#[derive(Debug, Clone)]
pub struct DownloadRules {
    /// At most this long from the sent request to the answer's status line (30 s).
    pub status_timeout: Duration,
    /// At most this long between two received chunks of the body (30 s).
    pub chunk_timeout: Duration,
    /// Attempts per download (6).
    pub attempts: u32,
    /// The backoff unit: the wait before attempt `k` is at most `unit × 2^(k−2)` (1 s).
    pub backoff_unit: Duration,
    /// The backoff cap (30 s).
    pub backoff_cap: Duration,
    /// An attempt that appended this many bytes resets the count to 1 (1 MiB).
    pub reset_after: u64,
    /// `memory_models_changed` at most this often while bytes arrive (1 s).
    pub progress_every: Duration,
    /// The person is not asked again sooner than this after **Not now** (60 s).
    pub ask_again_after: Duration,
    /// Test seam: a write that would carry the partial file past this many bytes fails with
    /// `ErrorKind::StorageFull` after writing up to it. `None` in the program.
    #[doc(hidden)]
    pub storage_full_after: Option<u64>,
}

impl Default for DownloadRules {
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-7
    fn default() -> DownloadRules {
        DownloadRules {
            status_timeout: Duration::from_secs(30),
            chunk_timeout: Duration::from_secs(30),
            attempts: 6,
            backoff_unit: Duration::from_secs(1),
            backoff_cap: Duration::from_secs(30),
            reset_after: 1024 * 1024,
            progress_every: Duration::from_secs(1),
            ask_again_after: Duration::from_secs(60),
            storage_full_after: None,
        }
    }
}

/// What [`MemoryModels::start`] needs.
pub struct ModelsDeps {
    pub source: Option<ModelsSource>,
    pub manifest: ModelManifest,
    /// `<data root>/models` (DSK-MD-1), the Memory Service's `models_dir`.
    pub models_dir: PathBuf,
    /// `<data root>/models.download` (DSK-MD-1).
    pub work_dir: PathBuf,
    /// `CoreInputs.http`: connect timeout 10 s, at most 3 redirects, `https` only.
    pub http: reqwest::Client,
    pub gate: Arc<dyn ModelsGate>,
    pub events: Arc<dyn ModelsEvents>,
    /// The core's shutdown token (DSK-MD-11).
    pub shutdown: CancellationToken,
    pub rules: DownloadRules,
}

struct State {
    view: MemoryModelsView,
    /// The running download's token and generation; at most one (DSK-MD-11).
    task: Option<(u64, CancellationToken)>,
    generation: u64,
    declined_at: Option<Instant>,
}

struct Shared {
    source: Option<ModelsSource>,
    manifest: ModelManifest,
    models_dir: PathBuf,
    work_dir: PathBuf,
    http: reqwest::Client,
    gate: Arc<dyn ModelsGate>,
    events: Arc<dyn ModelsEvents>,
    shutdown: CancellationToken,
    rules: DownloadRules,
    state: Mutex<State>,
}

/// The models' state and the download (DSK-MD-3 to DSK-MD-11). `Clone`.
#[derive(Clone)]
pub struct MemoryModels {
    shared: Arc<Shared>,
}

/// How one download ended.
enum Ended {
    /// Cancelled by **Stop**, quit, or a newer generation: nothing more to change.
    Cancelled,
    Failed(&'static str),
    Ready,
}

impl MemoryModels {
    /// DSK-MD-4: the state at start, then, in the background, the clean-up of the working folder, and
    /// the watch of `models_wanted` (DSK-MD-5). Missing models are asked about at once (DSK-MD-15);
    /// nothing is downloaded at start.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-4
    pub fn start(deps: ModelsDeps) -> MemoryModels {
        let view = match &deps.source {
            None => MemoryModelsView {
                state: MemoryModelsState::NotInBuild,
                size_bytes: 0,
                host: String::new(),
                received_bytes: 0,
                failure: None,
                ask: false,
            },
            Some(source) => {
                let ready = deps.gate.models_ready();
                MemoryModelsView {
                    state: if ready {
                        MemoryModelsState::Ready
                    } else {
                        MemoryModelsState::Missing
                    },
                    size_bytes: source.bytes,
                    host: host_of(&source.url),
                    received_bytes: if ready {
                        source.bytes
                    } else {
                        file_len(&part_path(&deps.work_dir, source))
                    },
                    failure: None,
                    // DSK-MD-15: missing models are offered at start, not only when memory is first used.
                    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-15
                    ask: !ready,
                }
            }
        };
        let models = MemoryModels {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    view,
                    task: None,
                    generation: 0,
                    declined_at: None,
                }),
                source: deps.source,
                manifest: deps.manifest,
                models_dir: deps.models_dir,
                work_dir: deps.work_dir,
                http: deps.http,
                gate: deps.gate,
                events: deps.events,
                shutdown: deps.shutdown,
                rules: deps.rules,
            }),
        };
        if let Some(source) = models.shared.source.clone() {
            let work_dir = models.shared.work_dir.clone();
            tokio::task::spawn_blocking(move || clean_working_folder(&work_dir, &source));
            // Subscribed before the task runs, so a refusal right after start is not missed.
            let wanted = models.shared.gate.models_wanted();
            let watcher = models.clone();
            tokio::spawn(async move { watcher.watch_wanted(wanted).await });
        }
        models
    }

    /// DSK-MD-3's view (`app.memory_models`).
    pub fn view(&self) -> MemoryModelsView {
        self.lock().view.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn emit(&self, view: &MemoryModelsView) {
        self.shared.events.changed(view);
    }

    /// DSK-MD-5: on each change of `models_wanted`, ask when the models are missing or failed, no
    /// download runs, the person has not been asked yet, and **Not now** was not answered in the last
    /// 60 seconds of this run.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-5
    async fn watch_wanted(&self, mut wanted: watch::Receiver<u64>) {
        loop {
            tokio::select! {
                _ = self.shared.shutdown.cancelled() => return,
                changed = wanted.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    self.memory_wanted();
                }
            }
        }
    }

    fn memory_wanted(&self) {
        let view = {
            let mut state = self.lock();
            let recent_no = state
                .declined_at
                .is_some_and(|at| at.elapsed() < self.shared.rules.ask_again_after);
            let waiting = matches!(
                state.view.state,
                MemoryModelsState::Missing | MemoryModelsState::Failed
            );
            if !waiting || state.task.is_some() || state.view.ask || recent_no {
                return;
            }
            state.view.ask = true;
            state.view.clone()
        };
        self.emit(&view);
    }

    /// `app.decline_memory_models` (**Not now**): `ask` false and the time recorded.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-12
    pub fn decline(&self) -> MemoryModelsView {
        let (view, changed) = {
            let mut state = self.lock();
            state.declined_at = Some(Instant::now());
            let changed = state.view.ask;
            state.view.ask = false;
            (state.view.clone(), changed)
        };
        if changed {
            self.emit(&view);
        }
        view
    }

    /// `app.download_memory_models` (DSK-MD-6): starts one background task in `missing` or
    /// `failed`; answers the current view in `downloading`, `checking` and `ready`; refused
    /// `DSK_MODELS_NOT_IN_BUILD` with no models input.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-6
    pub fn download(&self) -> Result<MemoryModelsView, &'static str> {
        let Some(source) = self.shared.source.clone() else {
            return Err(MODELS_NOT_IN_BUILD);
        };
        let (view, generation, cancel) = {
            let mut state = self.lock();
            match state.view.state {
                MemoryModelsState::NotInBuild => return Err(MODELS_NOT_IN_BUILD),
                MemoryModelsState::Downloading
                | MemoryModelsState::Checking
                | MemoryModelsState::Ready => return Ok(state.view.clone()),
                MemoryModelsState::Missing | MemoryModelsState::Failed => {}
            }
            state.generation += 1;
            let generation = state.generation;
            let cancel = self.shared.shutdown.child_token();
            state.task = Some((generation, cancel.clone()));
            state.view.state = MemoryModelsState::Downloading;
            state.view.failure = None;
            state.view.ask = false;
            state.view.received_bytes = file_len(&part_path(&self.shared.work_dir, &source));
            (state.view.clone(), generation, cancel)
        };
        self.emit(&view);
        let models = self.clone();
        tokio::spawn(async move {
            let ended = tokio::select! {
                _ = cancel.cancelled() => Ended::Cancelled,
                ended = models.run(&source, generation) => ended,
            };
            models.finish(&source, generation, ended);
        });
        Ok(view)
    }

    /// `app.stop_memory_models_download` (DSK-MD-11): while `downloading`, cancels the task, keeps
    /// the partial file and sets `missing`; otherwise answers the view and does nothing.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-11
    pub fn stop(&self) -> MemoryModelsView {
        let view = {
            let mut state = self.lock();
            if state.view.state != MemoryModelsState::Downloading {
                return state.view.clone();
            }
            if let Some((_, cancel)) = state.task.take() {
                cancel.cancel();
            }
            state.generation += 1;
            state.view.state = MemoryModelsState::Missing;
            state.view.ask = false;
            if let Some(source) = &self.shared.source {
                state.view.received_bytes = file_len(&part_path(&self.shared.work_dir, source));
            }
            state.view.clone()
        };
        self.emit(&view);
        view
    }

    /// Changes the view when `generation` is still the running download's; emits on a change of
    /// state or `ask`, or when `progress` is set.
    fn update(&self, generation: u64, progress: bool, change: impl FnOnce(&mut MemoryModelsView)) {
        let view = {
            let mut state = self.lock();
            if state.task.as_ref().map(|(g, _)| *g) != Some(generation) {
                return;
            }
            let before = (state.view.state, state.view.ask);
            change(&mut state.view);
            if !progress && before == (state.view.state, state.view.ask) {
                return;
            }
            state.view.clone()
        };
        self.emit(&view);
    }

    /// The end of one download: `ready`, or `failed` with its code (DSK-MD-6 to DSK-MD-10).
    fn finish(&self, source: &ModelsSource, generation: u64, ended: Ended) {
        let view = {
            let mut state = self.lock();
            if state.task.as_ref().map(|(g, _)| *g) != Some(generation) {
                return;
            }
            state.task = None;
            match ended {
                Ended::Cancelled => return,
                Ended::Ready => {
                    state.view.state = MemoryModelsState::Ready;
                    state.view.received_bytes = source.bytes;
                    state.view.failure = None;
                    state.view.ask = false;
                }
                Ended::Failed(code) => {
                    tracing::warn!(component = "desktop", code, "memory_models.download_failed");
                    state.view.state = MemoryModelsState::Failed;
                    state.view.failure = Some(code.to_owned());
                    state.view.received_bytes = file_len(&part_path(&self.shared.work_dir, source));
                }
            }
            state.view.clone()
        };
        self.emit(&view);
    }

    /// DSK-MD-6 to DSK-MD-10, until `ready` or a failure.
    async fn run(&self, source: &ModelsSource, generation: u64) -> Ended {
        let part = part_path(&self.shared.work_dir, source);
        if let Err(e) = tokio::fs::create_dir_all(&self.shared.work_dir).await {
            return Ended::Failed(write_code(&e));
        }
        tracing::info!(
            component = "desktop",
            resumed_from = file_len(&part),
            "memory_models.download_started"
        );
        if file_len(&part) < source.bytes {
            if let Err(code) = self.fetch(source, &part, generation).await {
                return Ended::Failed(code);
            }
        }
        self.update(generation, false, |view| {
            view.state = MemoryModelsState::Checking;
            view.received_bytes = view.size_bytes;
        });
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-9
        let checked = {
            let part = part.clone();
            let source = source.clone();
            let manifest = self.shared.manifest.clone();
            let models_dir = self.shared.models_dir.clone();
            let work_dir = self.shared.work_dir.clone();
            tokio::task::spawn_blocking(move || {
                check_and_place(&part, &source, &manifest, &models_dir, &work_dir)
            })
            .await
            .unwrap_or(Err(MODELS_WRITE_FAILED))
        };
        if let Err(code) = checked {
            return Ended::Failed(code);
        }
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-10
        if !self.shared.gate.recheck_models().await {
            return Ended::Failed(MODELS_MISMATCH);
        }
        tracing::info!(component = "desktop", "memory_models.ready");
        Ended::Ready
    }

    /// The attempts of DSK-MD-6 and DSK-MD-7, until the partial file holds `size_bytes` bytes.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-7
    async fn fetch(
        &self,
        source: &ModelsSource,
        part: &Path,
        generation: u64,
    ) -> Result<(), &'static str> {
        let rules = &self.shared.rules;
        let mut attempt: u32 = 1;
        loop {
            // Exponential backoff with full jitter. A stop or quit ends the wait: the caller's
            // `select!` drops this future at the token.
            if attempt >= 2 {
                let wait = backoff_cap(rules, attempt);
                let millis = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
                tokio::time::sleep(Duration::from_millis(rand::random_range(0..=millis))).await;
            }
            let before = file_len(part);
            let outcome = self.attempt(source, part, generation).await;
            let appended = file_len(part).saturating_sub(before);
            let failure = match outcome {
                Attempt::Complete => return Ok(()),
                Attempt::Stop(code) => return Err(code),
                Attempt::Retry(failure) => failure,
            };
            tracing::warn!(
                component = "desktop",
                attempt,
                status = failure.log_status().as_str(),
                "memory_models.attempt_failed"
            );
            if appended >= rules.reset_after {
                attempt = 1;
            }
            attempt += 1;
            if attempt > rules.attempts {
                let none_received = file_len(part) == 0;
                return Err(
                    if none_received
                        && matches!(failure, Failure::Connect | Failure::ConnectTimeout)
                    {
                        MODELS_OFFLINE
                    } else {
                        MODELS_INTERRUPTED
                    },
                );
            }
        }
    }

    /// One `GET` of the zip (DSK-MD-6, DSK-MD-8).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-6
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-8
    async fn attempt(&self, source: &ModelsSource, part: &Path, generation: u64) -> Attempt {
        let rules = &self.shared.rules;
        let have = file_len(part);
        let mut request = self.shared.http.get(source.url.clone());
        if have > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let response = match tokio::time::timeout(rules.status_timeout, request.send()).await {
            Err(_) => return Attempt::Retry(Failure::Timeout),
            Ok(Err(error)) => return send_failure(&error),
            Ok(Ok(response)) => response,
        };
        let status = response.status().as_u16();
        let append = match status {
            206 => {
                let range = response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(content_range);
                // A `206` with no readable range is taken as one starting elsewhere.
                let (start, total) = range.unwrap_or((u64::MAX, source.bytes));
                if total != source.bytes {
                    remove_file(part);
                    return Attempt::Stop(MODELS_MISMATCH);
                }
                if start != have {
                    if let Err(e) = truncate(part) {
                        return Attempt::Stop(write_code(&e));
                    }
                    return Attempt::Retry(Failure::Status(206));
                }
                true
            }
            200 => {
                if response.content_length().is_some_and(|n| n != source.bytes) {
                    remove_file(part);
                    return Attempt::Stop(MODELS_MISMATCH);
                }
                false
            }
            416 => {
                remove_file(part);
                return Attempt::Retry(Failure::Status(416));
            }
            401 | 403 | 404 | 407 | 410 | 451 => return Attempt::Stop(MODELS_BLOCKED),
            408 | 429 | 500..=599 => return Attempt::Retry(Failure::Status(status)),
            _ => return Attempt::Stop(MODELS_BLOCKED),
        };
        // A plain file, written chunk by chunk before the next await, so the partial file's length
        // is what was received whenever an attempt ends.
        let file = if append {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(part)
        } else {
            std::fs::File::create(part)
        };
        let mut file = match file {
            Ok(file) => file,
            Err(e) => return Attempt::Stop(write_code(&e)),
        };
        let mut written = if append { have } else { 0 };
        let mut response = response;
        let mut last_progress = Instant::now();
        loop {
            let chunk = match tokio::time::timeout(rules.chunk_timeout, response.chunk()).await {
                Err(_) => return Attempt::Retry(Failure::Timeout),
                Ok(Err(_)) => return Attempt::Retry(Failure::Short),
                Ok(Ok(None)) => break,
                Ok(Ok(Some(chunk))) => chunk,
            };
            let length = chunk.len() as u64;
            if written + length > source.bytes {
                drop(file);
                remove_file(part);
                return Attempt::Stop(MODELS_MISMATCH);
            }
            if let Err(e) = self.write_chunk(&mut file, written, &chunk) {
                return Attempt::Stop(write_code(&e));
            }
            written += length;
            let progress = last_progress.elapsed() >= rules.progress_every;
            if progress {
                last_progress = Instant::now();
            }
            self.update(generation, progress, |view| view.received_bytes = written);
        }
        if let Err(e) = file.flush() {
            return Attempt::Stop(write_code(&e));
        }
        if written < source.bytes {
            return Attempt::Retry(Failure::Short);
        }
        Attempt::Complete
    }

    /// One chunk appended to the partial file, with the test seam of a full disk.
    fn write_chunk(
        &self,
        file: &mut std::fs::File,
        written: u64,
        chunk: &[u8],
    ) -> std::io::Result<()> {
        if let Some(limit) = self.shared.rules.storage_full_after {
            if written + chunk.len() as u64 > limit {
                let room = usize::try_from(limit.saturating_sub(written)).unwrap_or(0);
                file.write_all(&chunk[..room.min(chunk.len())])?;
                file.flush()?;
                return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
            }
        }
        file.write_all(chunk)
    }
}

/// How one attempt ended.
enum Attempt {
    Complete,
    Retry(Failure),
    Stop(&'static str),
}

/// Why an attempt is retried; `status` of `memory_models.attempt_failed`.
#[derive(Debug, Clone, Copy)]
enum Failure {
    Connect,
    ConnectTimeout,
    Timeout,
    Short,
    Status(u16),
}

impl Failure {
    fn log_status(self) -> String {
        match self {
            Failure::Connect => "connect".to_owned(),
            Failure::ConnectTimeout | Failure::Timeout => "timeout".to_owned(),
            Failure::Short => "short".to_owned(),
            Failure::Status(status) => status.to_string(),
        }
    }
}

/// A send that failed before a status line: a refused redirect or certificate stops; a connection
/// failure or a time-out is retried.
fn send_failure(error: &reqwest::Error) -> Attempt {
    if error.is_redirect() || certificate_refused(error) {
        return Attempt::Stop(MODELS_BLOCKED);
    }
    if error.is_connect() && error.is_timeout() {
        return Attempt::Retry(Failure::ConnectTimeout);
    }
    if error.is_timeout() {
        return Attempt::Retry(Failure::Timeout);
    }
    Attempt::Retry(Failure::Connect)
}

/// DSK-MD-14.
fn certificate_refused(error: &reqwest::Error) -> bool {
    let mut cause: Option<&dyn std::error::Error> = Some(error);
    while let Some(current) = cause {
        if current.to_string().to_lowercase().contains("certificate") {
            return true;
        }
        cause = current.source();
    }
    false
}

/// The longest wait before attempt `k` (`k` ≥ 2): `min(cap, unit × 2^(k−2))`.
fn backoff_cap(rules: &DownloadRules, attempt: u32) -> Duration {
    let factor = 1u32
        .checked_shl(attempt.saturating_sub(2))
        .unwrap_or(u32::MAX);
    rules
        .backoff_unit
        .checked_mul(factor)
        .unwrap_or(rules.backoff_cap)
        .min(rules.backoff_cap)
}

/// `bytes <start>-<end>/<total>` → `(start, total)`.
fn content_range(value: &str) -> Option<(u64, u64)> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    Some((start.trim().parse().ok()?, total.trim().parse().ok()?))
}

/// The host name of the address, as written in it.
fn host_of(url: &reqwest::Url) -> String {
    url.host_str().unwrap_or_default().to_owned()
}

/// `<data root>/models.download/<sha256>.zip.part`.
fn part_path(work_dir: &Path, source: &ModelsSource) -> PathBuf {
    work_dir.join(format!("{}.zip.part", source.sha256))
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn remove_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn truncate(path: &Path) -> std::io::Result<()> {
    std::fs::File::create(path).map(|_| ())
}

/// A write error's code: a full disk, or any other (DSK-MD-8).
fn write_code(error: &std::io::Error) -> &'static str {
    if error.kind() == std::io::ErrorKind::StorageFull {
        MODELS_DISK_FULL
    } else {
        MODELS_WRITE_FAILED
    }
}

/// DSK-MD-4's clean-up: every `unpack-` and `old-` entry, and every partial file of another zip.
fn clean_working_folder(work_dir: &Path, source: &ModelsSource) {
    let Ok(entries) = std::fs::read_dir(work_dir) else {
        return;
    };
    let keep = format!("{}.zip.part", source.sha256);
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with("unpack-") || name.starts_with("old-") {
            let _ = std::fs::remove_dir_all(&path).or_else(|_| std::fs::remove_file(&path));
        } else if name.ends_with(".zip.part") && name != keep {
            remove_file(&path);
        }
    }
}

/// The SHA-256 of a file, as 64 lowercase hexadecimal characters.
fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(ordin_types::num::Sha256Hex::from_digest(&hasher.finalize().into()).0)
}

/// DSK-MD-9 and DSK-MD-10's renames, on a blocking thread.
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-9
// spec: ordin-desktop/components/OrdinDesktop#DSK-MD-10
fn check_and_place(
    part: &Path,
    source: &ModelsSource,
    manifest: &ModelManifest,
    models_dir: &Path,
    work_dir: &Path,
) -> Result<(), &'static str> {
    // 1. The zip's fingerprint; nothing is unpacked from a zip that differs.
    match sha256_of(part) {
        Ok(digest) if digest == source.sha256 => {}
        _ => {
            remove_file(part);
            return Err(MODELS_MISMATCH);
        }
    }
    // 2 and 3. Unpack the manifest's files and the licences, then check each file.
    let unpack = work_dir.join(format!("unpack-{}", uuid::Uuid::now_v7()));
    let unpacked = unpack_and_check(part, manifest, &unpack);
    if let Err(failure) = unpacked {
        let _ = std::fs::remove_dir_all(&unpack);
        return Err(match failure {
            Unpack::Mismatch => {
                remove_file(part);
                MODELS_MISMATCH
            }
            Unpack::Write(code) => code,
        });
    }
    // DSK-MD-10: the old folder aside, the new one in, then the old one and the partial file gone.
    let old = work_dir.join(format!("old-{}", uuid::Uuid::now_v7()));
    let had_old = models_dir.exists();
    if had_old && std::fs::rename(models_dir, &old).is_err() {
        let _ = std::fs::remove_dir_all(&unpack);
        return Err(MODELS_WRITE_FAILED);
    }
    if std::fs::rename(&unpack, models_dir).is_err() {
        if had_old {
            let _ = std::fs::rename(&old, models_dir);
        }
        let _ = std::fs::remove_dir_all(&unpack);
        return Err(MODELS_WRITE_FAILED);
    }
    if had_old {
        let _ = std::fs::remove_dir_all(&old);
    }
    remove_file(part);
    Ok(())
}

enum Unpack {
    /// A manifest path missing or twice, a file's fingerprint differs, or the zip is unreadable
    /// (DSK-MD-13).
    Mismatch,
    /// A write error's code (`DSK_MODELS_DISK_FULL` or `DSK_MODELS_WRITE_FAILED`).
    Write(&'static str),
}

fn unpack_and_check(part: &Path, manifest: &ModelManifest, unpack: &Path) -> Result<(), Unpack> {
    let write = |e: std::io::Error| Unpack::Write(write_code(&e));
    let file = std::fs::File::open(part).map_err(|_| Unpack::Mismatch)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| Unpack::Mismatch)?;
    std::fs::create_dir_all(unpack).map_err(write)?;
    let wanted: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    let mut seen: Vec<String> = Vec::new();
    let mut licences_written = false;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|_| Unpack::Mismatch)?;
        let name = entry.name().map_err(|_| Unpack::Mismatch)?.into_owned();
        if entry.is_dir() {
            continue;
        }
        // Only the manifest's paths and the licences are written, so no entry name can place a file
        // outside the folder; every other entry (`SHA256SUMS` among them) is skipped.
        let is_model = wanted.contains(&name.as_str());
        if !is_model && (name != LICENCES || licences_written) {
            continue;
        }
        if is_model {
            if seen.contains(&name) {
                return Err(Unpack::Mismatch);
            }
            seen.push(name.clone());
        } else {
            licences_written = true;
        }
        let path = unpack.join(&name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(write)?;
        }
        let mut out = std::fs::File::create(&path).map_err(write)?;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buffer).map_err(|_| Unpack::Mismatch)?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read]).map_err(write)?;
        }
        out.flush().map_err(write)?;
        out.sync_all().map_err(write)?;
    }
    if seen.len() != wanted.len() {
        return Err(Unpack::Mismatch);
    }
    for file in &manifest.files {
        match sha256_of(&unpack.join(&file.path)) {
            Ok(digest) if digest == file.sha256 => {}
            _ => return Err(Unpack::Mismatch),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "memory_models_tests.rs"]
mod tests;
