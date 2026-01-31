//! Speech to text with whisper.cpp (TranscriptionService.md Internal Logic C4–C6).

use std::ffi::c_void;
use std::path::Path;
use std::sync::Once;

use tokio_util::sync::CancellationToken;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::audio::TrackAudio;
use super::error::TranscriptionError;
use super::parse::Raw;

/// Code of `Engine` when the model file could not be loaded.
const LOAD_FAILED: i32 = -1;
/// Code of `Engine` when a recognition state could not be made.
const STATE_FAILED: i32 = -2;

/// whisper.cpp writes its own log lines to standard error; they are routed to a hook that
/// drops them, so nothing of the audio's content can reach a log.
fn silence_engine_logs() {
    static ONCE: Once = Once::new();
    ONCE.call_once(whisper_rs::install_logging_hooks);
}

/// whisper.cpp asks this between steps of its work; `data` is the `CancellationToken` that
/// `recognise` keeps alive until `full` returns.
unsafe extern "C" fn should_abort(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    // SAFETY: `data` was made from `&CancellationToken` in `recognise`, which outlives the
    // `full` call that invokes this function; the token is only read.
    #[allow(unsafe_code)]
    let token = unsafe { &*(data as *const CancellationToken) };
    token.is_cancelled()
}

/// The outcome of recognition: utterances before normalisation and the detected language.
pub(crate) struct Recognised {
    pub raw: Vec<Raw>,
    pub language: Option<String>,
}

/// C4–C6 for every track. `progress(done_ms)` reports recognition across all tracks, whose
/// total is `tracks.len() × duration_ms`.
pub(crate) fn recognise(
    model: &Path,
    tracks: &[TrackAudio],
    duration_ms: u64,
    cancel: &CancellationToken,
    progress: std::sync::Arc<dyn Fn(u64) + Send + Sync>,
) -> Result<Recognised, TranscriptionError> {
    silence_engine_logs();
    // C4: the default uses the GPU only when a GPU feature is on, which is Metal on Apple
    // silicon (Cargo.toml); the CPU elsewhere.
    let context = WhisperContext::new_with_params(model, WhisperContextParameters::default())
        .map_err(|_| TranscriptionError::Engine(LOAD_FAILED))?;
    let threads = engine_threads();

    let mut raw = Vec::new();
    let mut language = None;
    for (index, track) in tracks.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(TranscriptionError::Cancelled);
        }
        if track.samples.is_empty() {
            continue;
        }
        let mut state = context
            .create_state()
            .map_err(|_| TranscriptionError::Engine(STATE_FAILED))?;
        let threshold = sound_threshold(&track.samples);
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(threads);
        // C5: language detection on, timestamps on, nothing printed.
        params.set_language(Some("auto"));
        params.set_no_timestamps(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        let base = index as u64 * duration_ms;
        let report = progress.clone();
        params.set_progress_callback_safe(move |percent: i32| {
            let percent = u64::try_from(percent.clamp(0, 100)).unwrap_or(0);
            report(base + duration_ms * percent / 100);
        });
        // SAFETY: `should_abort` reads the token through the pointer only while `full` runs,
        // and `cancel` outlives that call.
        #[allow(unsafe_code)]
        unsafe {
            params.set_abort_callback(Some(should_abort));
            params.set_abort_callback_user_data(cancel as *const CancellationToken as *mut c_void);
        }
        // whisper.cpp reports in steps of 5 %; say at once that this track has started.
        progress(base);
        let result = state.full(params, &track.samples);
        if cancel.is_cancelled() {
            return Err(TranscriptionError::Cancelled);
        }
        if let Err(error) = result {
            return Err(TranscriptionError::Engine(engine_code(&error)));
        }
        if language.is_none() {
            language = whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_owned);
        }
        // C6: each segment becomes one utterance; whisper.cpp times are in centiseconds.
        for segment in state.as_iter() {
            let text = segment
                .to_str_lossy()
                .map(|t| t.into_owned())
                .unwrap_or_default();
            let start = u64::try_from(segment.start_timestamp()).unwrap_or(0) * 10;
            let end = u64::try_from(segment.end_timestamp()).unwrap_or(0) * 10;
            let start = first_sound_ms(&track.samples, start, end, threshold);
            raw.push(Raw {
                start,
                end: Some(end.max(start)),
                speaker: track.speaker.map(str::to_owned),
                text,
            });
        }
        progress(base + duration_ms);
    }
    Ok(Recognised { raw, language })
}

/// One engine thread per physical core. whisper.cpp's threads wait for each other at every step,
/// so a thread on each hardware thread of a two-way SMT processor, or on a core another program
/// is using, stalls them all: on the 16-core reference server 32 threads took over eight times as
/// long as 16. Half the hardware threads is one per core with two-way SMT, and the performance
/// cores of most Apple silicon.
fn engine_threads() -> i32 {
    let hardware = std::thread::available_parallelism().map_or(4, |n| n.get());
    i32::try_from((hardware / 2).max(1)).unwrap_or(4)
}

/// Samples in one 20 ms frame at 16 kHz.
const FRAME: usize = 320;

/// The loudness (RMS) a frame needs to count as sound: a tenth of the loudest frame of the track,
/// and never below a noise floor.
fn sound_threshold(samples: &[f32]) -> f32 {
    let loudest = samples.chunks(FRAME).map(rms).fold(0.0f32, f32::max);
    (loudest * 0.1).max(0.005)
}

fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
}

/// whisper.cpp starts a segment that follows silence at the start of its 30-second window, so a
/// person who is silent for the first seconds of a track would appear to speak first. The start
/// moves to the first frame with sound between `start_ms` and `end_ms`; it stays unchanged when
/// none is found.
pub(crate) fn first_sound_ms(samples: &[f32], start_ms: u64, end_ms: u64, threshold: f32) -> u64 {
    let to_index = |ms: u64| {
        usize::try_from(ms * 16)
            .unwrap_or(usize::MAX)
            .min(samples.len())
    };
    let (from, to) = (to_index(start_ms), to_index(end_ms));
    if from >= to {
        return start_ms;
    }
    samples[from..to]
        .chunks(FRAME)
        .position(|frame| rms(frame) >= threshold)
        .map_or(start_ms, |frame| start_ms + (frame * FRAME) as u64 / 16)
}

/// The whisper.cpp result code when there is one.
fn engine_code(error: &whisper_rs::WhisperError) -> i32 {
    match error {
        whisper_rs::WhisperError::GenericError(code) => *code,
        _ => STATE_FAILED,
    }
}
