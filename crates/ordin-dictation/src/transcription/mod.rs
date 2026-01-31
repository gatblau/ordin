//! The Transcription Service (TranscriptionService.md): one meeting file to a `Transcript`, on this
//! computer only: transcript files (Internal Logic A), the speech model (B) and recordings (C).

mod audio;
mod error;
mod machine;
mod models;
mod parse;
mod recognise;
mod types;

#[cfg(test)]
mod machine_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use ordin_localisation::Localisation;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub use error::{TranscriptionError, ALL_CODES};
pub use machine::{machine_facts, recommend, recommended_model, MachineFacts};
pub use types::{
    DownloadProgress, SpeakerTracks, SpeechModelChoice, SpeechModelInfo, TranscribeProgress,
    Transcript, TranscriptSourceKind, Utterance,
};

use models::{DownloadTuning, ModelEntry, OneAtATime};

/// `read_transcript` refuses a file larger than this.
pub const MAX_TRANSCRIPT_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// `transcribe_audio` refuses a file larger than this.
pub const MAX_AUDIO_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// `transcribe_audio` refuses a recording longer than four hours.
pub const MAX_AUDIO_DURATION_MS: u64 = 14_400_000;
/// Cancel stops a transcription within this time.
pub const CANCEL_DEADLINE_MS: u64 = 5_000;
/// `Transcript.source_name`: at most 255 characters.
const MAX_SOURCE_NAME_CHARS: usize = 255;
/// Code of `Engine` when the recognition thread itself failed.
const THREAD_FAILED: i32 = -3;

/// One service per desktop program with `minutes`.
pub struct TranscriptionService {
    models_folder: PathBuf,
    // The service's sentences are rendered by its callers from the error codes; kept for the
    // interface of TranscriptionService.md.
    #[allow(dead_code)]
    localisation: Arc<Localisation>,
    http: reqwest::Client,
    /// The bundled list; empty only if the bundled file is broken, which its unit test forbids.
    models: Vec<ModelEntry>,
    tuning: DownloadTuning,
    downloading: Arc<AtomicBool>,
    transcribing: Arc<AtomicBool>,
}

/// The kinds of transcript file, by lower-case extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptKind {
    Vtt,
    Docx,
    Txt,
}

impl TranscriptKind {
    fn of(path: &Path) -> Option<TranscriptKind> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "vtt" => Some(TranscriptKind::Vtt),
            "docx" => Some(TranscriptKind::Docx),
            "txt" => Some(TranscriptKind::Txt),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            TranscriptKind::Vtt => "vtt",
            TranscriptKind::Docx => "docx",
            TranscriptKind::Txt => "txt",
        }
    }
}

/// A2–A5 over the bytes of a file of `kind`.
fn utterances_of(kind: TranscriptKind, bytes: &[u8]) -> Result<Vec<Utterance>, TranscriptionError> {
    let raw = match kind {
        TranscriptKind::Vtt => {
            let text =
                std::str::from_utf8(bytes).map_err(|_| TranscriptionError::TranscriptMalformed)?;
            parse::parse_vtt(text)?
        }
        TranscriptKind::Docx => parse::parse_lines(&parse::docx_lines(bytes)?),
        TranscriptKind::Txt => parse::parse_lines(&parse::txt_lines(bytes)?),
    };
    parse::normalise(raw)
}

impl TranscriptionService {
    pub fn new(
        models_folder: PathBuf,
        localisation: Arc<Localisation>,
        http: reqwest::Client,
    ) -> Arc<Self> {
        let models = models::parse_list(models::BUNDLED_LIST).unwrap_or_else(|reason| {
            tracing::error!(
                component = "transcription",
                reason,
                "transcription.model_list_invalid"
            );
            Vec::new()
        });
        Self::with_models(
            models_folder,
            localisation,
            http,
            models,
            DownloadTuning::default(),
        )
    }

    fn with_models(
        models_folder: PathBuf,
        localisation: Arc<Localisation>,
        http: reqwest::Client,
        models: Vec<ModelEntry>,
        tuning: DownloadTuning,
    ) -> Arc<Self> {
        // Data Model: part files left by a crash are deleted at start.
        models::remove_stale_parts(&models_folder);
        Arc::new(TranscriptionService {
            models_folder,
            localisation,
            http,
            models,
            tuning,
            downloading: Arc::new(AtomicBool::new(false)),
            transcribing: Arc::new(AtomicBool::new(false)),
        })
    }

    fn entry(&self, choice: SpeechModelChoice) -> Result<&ModelEntry, TranscriptionError> {
        self.models
            .iter()
            .find(|m| m.choice == choice)
            .ok_or(TranscriptionError::ModelMissing)
    }

    /// Both models, from `speech-models.json`; never touches the network.
    pub async fn models(&self) -> Vec<SpeechModelInfo> {
        self.models
            .iter()
            .map(|m| models::info(m, &self.models_folder))
            .collect()
    }

    /// Called only after the person agreed. Downloads to `<file>.part`, verifies SHA-256, renames
    /// (Internal Logic B).
    pub async fn download_model(
        &self,
        choice: SpeechModelChoice,
        progress: watch::Sender<DownloadProgress>,
        cancel: CancellationToken,
    ) -> Result<SpeechModelInfo, TranscriptionError> {
        let result = self.download_inner(choice, &progress, &cancel).await;
        log_failure(&result);
        result
    }

    async fn download_inner(
        &self,
        choice: SpeechModelChoice,
        progress: &watch::Sender<DownloadProgress>,
        cancel: &CancellationToken,
    ) -> Result<SpeechModelInfo, TranscriptionError> {
        // B1.
        let entry = self.entry(choice)?;
        let info = models::info(entry, &self.models_folder);
        if info.present {
            return Ok(info);
        }
        // B5.
        let _guard = OneAtATime::take(&self.downloading)?;
        models::download(
            entry,
            &self.models_folder,
            &self.http,
            &self.tuning,
            progress,
            cancel,
        )
        .await?;
        Ok(models::info(entry, &self.models_folder))
    }

    /// Deletes the model file; Ok when absent.
    pub async fn remove_model(&self, choice: SpeechModelChoice) -> Result<(), TranscriptionError> {
        let entry = self.entry(choice)?;
        let result =
            match tokio::fs::remove_file(models::model_path(entry, &self.models_folder)).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                // SPEC-GAP-1: the Error Table has no row for a model file that cannot be deleted;
                // the nearest is "file cannot be opened or read".
                Err(_) => Err(TranscriptionError::FileUnreadable),
            };
        log_failure(&result);
        result
    }

    /// `.m4a`, `.mp3`, `.wav`, `.mkv` only. Needs the chosen model present (Internal Logic C).
    pub async fn transcribe_audio(
        &self,
        path: &Path,
        source_kind: TranscriptSourceKind,
        model: SpeechModelChoice,
        tracks: SpeakerTracks,
        progress: watch::Sender<TranscribeProgress>,
        cancel: CancellationToken,
    ) -> Result<Transcript, TranscriptionError> {
        let result = self
            .transcribe_inner(path, source_kind, model, tracks, progress, cancel)
            .await;
        log_failure(&result);
        result
    }

    async fn transcribe_inner(
        &self,
        path: &Path,
        source_kind: TranscriptSourceKind,
        model: SpeechModelChoice,
        tracks: SpeakerTracks,
        progress: watch::Sender<TranscribeProgress>,
        cancel: CancellationToken,
    ) -> Result<Transcript, TranscriptionError> {
        let started = Instant::now();
        // C1: type, size, model, one at a time.
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !matches!(extension.as_str(), "m4a" | "mp3" | "wav" | "mkv") {
            return Err(TranscriptionError::FormatUnsupported);
        }
        let size = tokio::fs::metadata(path)
            .await
            .map_err(|_| TranscriptionError::FileUnreadable)?
            .len();
        if size > MAX_AUDIO_FILE_BYTES {
            return Err(TranscriptionError::FileTooLarge);
        }
        let entry = self.entry(model)?;
        let model_file = models::model_path(entry, &self.models_folder);
        if !model_file.is_file() {
            return Err(TranscriptionError::ModelMissing);
        }
        let guard = OneAtATime::take(&self.transcribing)?;

        // C2–C6 on a blocking thread; the audio is dropped when it ends (C7).
        let path_owned = path.to_path_buf();
        let progress = Arc::new(progress);
        let worker_cancel = cancel.clone();
        let worker = tokio::task::spawn_blocking(move || {
            // The thread keeps the one-at-a-time guard until the engine has stopped.
            let _guard = guard;
            let cancel = worker_cancel;
            let decoding = progress.clone();
            let (audio, duration_ms) = audio::decode(
                &path_owned,
                tracks,
                MAX_AUDIO_DURATION_MS,
                &cancel,
                &move |done_ms, total_ms| {
                    decoding.send_replace(TranscribeProgress::Decoding { done_ms, total_ms });
                },
            )?;
            let total_ms = duration_ms * audio.len() as u64;
            let recognising = progress.clone();
            let recognised = recognise::recognise(
                &model_file,
                &audio,
                duration_ms,
                &cancel,
                Arc::new(move |done_ms| {
                    recognising.send_replace(TranscribeProgress::Recognising {
                        done_ms: done_ms.min(total_ms),
                        total_ms,
                    });
                }),
            )?;
            Ok::<_, TranscriptionError>((recognised, duration_ms))
        });
        // whisper.cpp checks for cancel only between 30-second windows, which can take longer
        // than CANCEL_DEADLINE_MS on a CPU; cancel answers at once and the thread finishes its
        // window alone, still holding the guard, so a new transcription meanwhile gets `Busy`.
        let recognised = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TranscriptionError::Cancelled),
            joined = worker => joined.map_err(|_| TranscriptionError::Engine(THREAD_FAILED))?,
        };
        let (recognised, duration_ms) = recognised?;

        // C6: normalise as A5; both tracks merged by start time.
        let utterances = parse::normalise(recognised.raw)?;
        let duration_ms = duration_ms.max(utterances.iter().map(|u| u.end_ms).max().unwrap_or(0));
        // C7.
        tracing::info!(
            component = "transcription",
            source_kind = source_kind_str(source_kind),
            model = model.as_str(),
            duration_ms,
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            utterances = utterances.len(),
            "transcription.done"
        );
        Ok(Transcript {
            format: 1,
            source_kind,
            source_name: source_name(path),
            language: recognised.language,
            duration_ms,
            utterances,
        })
    }

    /// `.vtt`, `.docx`, `.txt` only (Internal Logic A). The file is opened read-only; no text, name
    /// or path is logged.
    pub async fn read_transcript(&self, path: &Path) -> Result<Transcript, TranscriptionError> {
        let result = read_transcript_file(path).await;
        log_failure(&result);
        result
    }
}

/// `transcription.failed` with the code only.
fn log_failure<T>(result: &Result<T, TranscriptionError>) {
    if let Err(error) = result {
        tracing::warn!(
            component = "transcription",
            code = ordin_types::CodedError::code(error),
            "transcription.failed"
        );
    }
}

fn source_kind_str(kind: TranscriptSourceKind) -> &'static str {
    match kind {
        TranscriptSourceKind::TranscriptFile => "transcript_file",
        TranscriptSourceKind::AudioFile => "audio_file",
        TranscriptSourceKind::Recording => "recording",
    }
}

/// The file name only, at most 255 characters.
fn source_name(path: &Path) -> String {
    path.file_name()
        .map(|n| {
            n.to_string_lossy()
                .chars()
                .take(MAX_SOURCE_NAME_CHARS)
                .collect()
        })
        .unwrap_or_default()
}

async fn read_transcript_file(path: &Path) -> Result<Transcript, TranscriptionError> {
    // A1: the type, then the size, then the read.
    let kind = TranscriptKind::of(path).ok_or(TranscriptionError::FormatUnsupported)?;
    let size = tokio::fs::metadata(path)
        .await
        .map_err(|_| TranscriptionError::FileUnreadable)?
        .len();
    if size > MAX_TRANSCRIPT_FILE_BYTES {
        return Err(TranscriptionError::FileTooLarge);
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| TranscriptionError::FileUnreadable)?;
    if bytes.len() as u64 > MAX_TRANSCRIPT_FILE_BYTES {
        return Err(TranscriptionError::FileTooLarge);
    }
    let utterances = tokio::task::spawn_blocking(move || utterances_of(kind, &bytes))
        .await
        .map_err(|_| TranscriptionError::FileUnreadable)??;
    let duration_ms = utterances.iter().map(|u| u.end_ms).max().unwrap_or(0);
    let source_name = source_name(path);
    // A6.
    tracing::info!(
        component = "transcription",
        kind = kind.as_str(),
        utterances = utterances.len(),
        duration_ms,
        "transcription.read"
    );
    Ok(Transcript {
        format: 1,
        source_kind: TranscriptSourceKind::TranscriptFile,
        source_name,
        language: None,
        duration_ms,
        utterances,
    })
}
