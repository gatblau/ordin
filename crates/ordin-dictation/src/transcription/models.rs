//! The speech models: the bundled list, the download with resume and checksum, removal
//! (TranscriptionService.md Internal Logic B).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::error::TranscriptionError;
use super::types::{DownloadProgress, SpeechModelChoice, SpeechModelInfo};

/// The bundled model list; its values come from the whisper.cpp project's published model files.
pub(crate) const BUNDLED_LIST: &str = include_str!("../../speech-models.json");

/// A read idle for this long counts as a failure and is retried.
pub(crate) const DOWNLOAD_TIMEOUT_S: u64 = 30;
/// Retries after the first attempt, with the backoff below.
pub(crate) const DOWNLOAD_RETRIES: usize = 3;
/// Free space required beyond the model's own size, in megabytes.
const SPACE_MARGIN_MB: u64 = 100;
const MB: u64 = 1024 * 1024;
const PART_SUFFIX: &str = ".part";

/// One entry of `speech-models.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct ModelEntry {
    pub choice: SpeechModelChoice,
    pub file_name: String,
    pub size_mb: u32,
    pub sha256: String,
    pub url: String,
}

#[derive(Deserialize)]
struct ModelList {
    format: u32,
    models: Vec<ModelEntry>,
}

/// Parses and checks a model list: format 1, both choices once each, `https` addresses,
/// 64 lower-case hex checksums, sizes 1–4,000 MB, plain file names.
pub(crate) fn parse_list(json: &str) -> Result<Vec<ModelEntry>, String> {
    let list: ModelList = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if list.format != 1 {
        return Err(format!("format {} is not 1", list.format));
    }
    for choice in [SpeechModelChoice::Larger, SpeechModelChoice::Smaller] {
        let count = list.models.iter().filter(|m| m.choice == choice).count();
        if count != 1 {
            return Err(format!("{} listed {count} times", choice.as_str()));
        }
    }
    for m in &list.models {
        if !m.url.starts_with("https://") {
            return Err(format!("{}: address is not https", m.file_name));
        }
        let hex = m.sha256.len() == 64
            && m.sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex {
            return Err(format!(
                "{}: checksum is not 64 lower-case hex",
                m.file_name
            ));
        }
        if !(1..=4000).contains(&m.size_mb) {
            return Err(format!(
                "{}: size {} MB out of range",
                m.file_name, m.size_mb
            ));
        }
        let plain = !m.file_name.is_empty()
            && !m.file_name.contains(['/', '\\'])
            && m.file_name != ".."
            && !m.file_name.ends_with(PART_SUFFIX);
        if !plain {
            return Err(format!("{:?}: not a plain file name", m.file_name));
        }
    }
    Ok(list.models)
}

/// The timings of a download; tests shorten them.
#[derive(Debug, Clone)]
pub(crate) struct DownloadTuning {
    pub read_timeout: Duration,
    /// One wait per retry, before ±20 % jitter.
    pub backoff: [Duration; DOWNLOAD_RETRIES],
}

impl Default for DownloadTuning {
    fn default() -> Self {
        DownloadTuning {
            read_timeout: Duration::from_secs(DOWNLOAD_TIMEOUT_S),
            backoff: [
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
            ],
        }
    }
}

/// Sets a flag for as long as it lives; the second holder gets `Busy`. It owns the flag so a
/// cancelled transcription's thread can keep it until the engine has really stopped.
pub(crate) struct OneAtATime(Arc<AtomicBool>);

impl OneAtATime {
    pub(crate) fn take(flag: &Arc<AtomicBool>) -> Result<OneAtATime, TranscriptionError> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| OneAtATime(flag.clone()))
            .map_err(|_| TranscriptionError::Busy)
    }
}

impl Drop for OneAtATime {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) fn info(entry: &ModelEntry, folder: &Path) -> SpeechModelInfo {
    SpeechModelInfo {
        choice: entry.choice,
        file_name: entry.file_name.clone(),
        size_mb: entry.size_mb,
        folder: folder.to_path_buf(),
        present: folder.join(&entry.file_name).is_file(),
    }
}

/// Deletes part files a crash left behind (Data Model). Failures are ignored: the next download
/// truncates its part file anyway.
pub(crate) fn remove_stale_parts(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_part = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(PART_SUFFIX));
        if is_part && path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Why one attempt stopped.
enum Stop {
    /// A network failure or an idle read: retried with `Range` resume.
    Retry,
    /// Not retried; the part file is deleted.
    Fail(TranscriptionError),
}

/// What the download has written so far.
struct PartFile {
    file: tokio::fs::File,
    hasher: Sha256,
    received: u64,
}

/// B2–B4 for one model. The caller holds the one-at-a-time guard and has checked presence.
pub(crate) async fn download(
    entry: &ModelEntry,
    folder: &Path,
    http: &reqwest::Client,
    tuning: &DownloadTuning,
    progress: &watch::Sender<DownloadProgress>,
    cancel: &CancellationToken,
) -> Result<(), TranscriptionError> {
    let started = Instant::now();
    tokio::fs::create_dir_all(folder)
        .await
        .map_err(|e| write_error(&e))?;
    // B2.
    let needed = (u64::from(entry.size_mb) + SPACE_MARGIN_MB) * MB;
    if free_bytes(folder).is_some_and(|free| free < needed) {
        return Err(TranscriptionError::NoSpace);
    }
    let part_path = folder.join(format!("{}{PART_SUFFIX}", entry.file_name));
    let final_path = folder.join(&entry.file_name);
    let file = tokio::fs::File::create(&part_path)
        .await
        .map_err(|e| write_error(&e))?;
    let mut part = PartFile {
        file,
        hasher: Sha256::new(),
        received: 0,
    };
    let result = fetch(entry, http, tuning, progress, cancel, &mut part).await;
    let result = match result {
        Ok(()) => finish(entry, part, &part_path, &final_path).await,
        Err(e) => {
            drop(part);
            Err(e)
        }
    };
    match &result {
        Ok(()) => tracing::info!(
            component = "transcription",
            choice = entry.choice.as_str(),
            size_mb = entry.size_mb,
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "transcription.model_downloaded"
        ),
        Err(_) => {
            let _ = tokio::fs::remove_file(&part_path).await;
        }
    }
    result
}

/// B3: the first attempt and up to `DOWNLOAD_RETRIES` more, each resuming where the last stopped.
async fn fetch(
    entry: &ModelEntry,
    http: &reqwest::Client,
    tuning: &DownloadTuning,
    progress: &watch::Sender<DownloadProgress>,
    cancel: &CancellationToken,
    part: &mut PartFile,
) -> Result<(), TranscriptionError> {
    let mut attempts = 0usize;
    loop {
        attempts += 1;
        let outcome = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TranscriptionError::Cancelled),
            outcome = attempt(entry, http, tuning, progress, part) => outcome,
        };
        match outcome {
            Ok(()) => return Ok(()),
            Err(Stop::Fail(e)) => return Err(e),
            Err(Stop::Retry) => {}
        }
        let Some(wait) = tuning.backoff.get(attempts - 1) else {
            tracing::warn!(
                component = "transcription",
                choice = entry.choice.as_str(),
                attempts,
                "transcription.download_failed"
            );
            return Err(TranscriptionError::DownloadFailed);
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TranscriptionError::Cancelled),
            _ = tokio::time::sleep(jitter(*wait)) => {}
        }
    }
}

/// One GET: from the start, or from `part.received` with `Range` when the server honours it.
async fn attempt(
    entry: &ModelEntry,
    http: &reqwest::Client,
    tuning: &DownloadTuning,
    progress: &watch::Sender<DownloadProgress>,
    part: &mut PartFile,
) -> Result<(), Stop> {
    let mut request = http.get(&entry.url);
    if part.received > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={}-", part.received));
    }
    let mut response = match tokio::time::timeout(tuning.read_timeout, request.send()).await {
        Ok(Ok(response)) => response,
        _ => return Err(Stop::Retry),
    };
    let status = response.status();
    if part.received > 0 && status == reqwest::StatusCode::OK {
        // The server ignored the range: start again from the first byte.
        part.file
            .set_len(0)
            .await
            .map_err(|e| Stop::Fail(write_error(&e)))?;
        part.file
            .seek(std::io::SeekFrom::Start(0))
            .await
            .map_err(|e| Stop::Fail(write_error(&e)))?;
        part.hasher = Sha256::new();
        part.received = 0;
    } else if !(status == reqwest::StatusCode::OK
        || (part.received > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT))
    {
        return Err(Stop::Retry);
    }
    let total_mb = entry.size_mb;
    loop {
        let chunk = match tokio::time::timeout(tuning.read_timeout, response.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => return Ok(()),
            _ => return Err(Stop::Retry),
        };
        part.file
            .write_all(&chunk)
            .await
            .map_err(|e| Stop::Fail(write_error(&e)))?;
        part.hasher.update(&chunk);
        let before_mb = part.received / MB;
        part.received += chunk.len() as u64;
        let received_mb = part.received / MB;
        if received_mb != before_mb {
            progress.send_replace(DownloadProgress {
                received_mb: u32::try_from(received_mb).unwrap_or(u32::MAX),
                total_mb,
            });
        }
    }
}

/// B4: checksum, `fsync`, rename.
async fn finish(
    entry: &ModelEntry,
    part: PartFile,
    part_path: &Path,
    final_path: &Path,
) -> Result<(), TranscriptionError> {
    let PartFile {
        mut file, hasher, ..
    } = part;
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if hex != entry.sha256 {
        return Err(TranscriptionError::ChecksumMismatch);
    }
    file.flush().await.map_err(|e| write_error(&e))?;
    file.sync_all().await.map_err(|e| write_error(&e))?;
    drop(file);
    tokio::fs::rename(part_path, final_path)
        .await
        .map_err(|e| write_error(&e))
}

/// A full disk is `NoSpace`; any other local write failure ends the download as failed.
fn write_error(error: &std::io::Error) -> TranscriptionError {
    if error.kind() == std::io::ErrorKind::StorageFull {
        TranscriptionError::NoSpace
    } else {
        TranscriptionError::DownloadFailed
    }
}

/// `wait` ±20 %, from the clock's nanoseconds (no secret depends on it).
fn jitter(wait: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // 0..=40 → 80 %..=120 %.
    let percent = 80 + u64::from(nanos % 41);
    let millis = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
    Duration::from_millis(millis.saturating_mul(percent) / 100)
}

/// Free bytes for this user in the file system holding `path`; `None` when it cannot be read,
/// in which case the download goes ahead and a full disk ends it as `NoSpace`.
#[cfg(unix)]
pub(crate) fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `c_path` is a valid NUL-terminated string and `stats` points to writable memory of
    // the right type; `statvfs` writes only into it.
    #[allow(unsafe_code)]
    let status = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if status != 0 {
        return None;
    }
    // SAFETY: `statvfs` returned 0, so it filled `stats`.
    #[allow(unsafe_code)]
    let stats = unsafe { stats.assume_init() };
    #[allow(clippy::useless_conversion)]
    let blocks = u64::from(stats.f_bavail);
    #[allow(clippy::useless_conversion)]
    let block_size = u64::from(stats.f_frsize);
    Some(blocks.saturating_mul(block_size))
}

/// Free bytes for this user in the volume holding `path`; `None` when it cannot be read.
#[cfg(windows)]
pub(crate) fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available: u64 = 0;
    // SAFETY: `wide` is NUL-terminated; the two optional outputs are null, which the function
    // accepts; `available` is a valid writable u64.
    #[allow(unsafe_code)]
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(available)
}

/// The model file of `entry` in `folder`.
pub(crate) fn model_path(entry: &ModelEntry, folder: &Path) -> PathBuf {
    folder.join(&entry.file_name)
}
