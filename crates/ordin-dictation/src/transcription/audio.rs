//! Recordings to 16 kHz mono samples, in memory only (TranscriptionService.md Internal Logic C2–C3).

use std::path::Path;
use std::time::{Duration, Instant};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as AudioError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tokio_util::sync::CancellationToken;

use super::error::TranscriptionError;
use super::types::SpeakerTracks;

/// The rate the speech engine takes.
pub(crate) const SAMPLE_RATE_HZ: u32 = 16_000;
/// Progress is sent at least this often.
pub(crate) const PROGRESS_INTERVAL_MS: u64 = 1_000;

/// One track ready for recognition.
pub(crate) struct TrackAudio {
    /// "You", "Others", or `None` when mixed.
    pub speaker: Option<&'static str>,
    pub samples: Vec<f32>,
}

/// C2–C3: probe, decode every audio track, mix or split, resample. `progress(done_ms, total_ms)`
/// is called at most once per `PROGRESS_INTERVAL_MS`.
pub(crate) fn decode(
    path: &Path,
    tracks: SpeakerTracks,
    max_duration_ms: u64,
    cancel: &CancellationToken,
    progress: &dyn Fn(u64, u64),
) -> Result<(Vec<TrackAudio>, u64), TranscriptionError> {
    let file = std::fs::File::open(path).map_err(|_| TranscriptionError::FileUnreadable)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(&extension.to_ascii_lowercase());
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|_| TranscriptionError::AudioUndecodable)?;
    let mut format = probed.format;

    // C2: the audio tracks in file order, a decoder for each.
    let mut decoders: Vec<(u32, Box<dyn Decoder>, Resampler)> = Vec::new();
    let mut known_ms = 0u64;
    for track in format.tracks() {
        let params = &track.codec_params;
        let Some(rate) = params.sample_rate else {
            continue;
        };
        if params.codec == CODEC_TYPE_NULL || rate == 0 {
            continue;
        }
        if let Some(frames) = params.n_frames {
            known_ms = known_ms.max(frames.saturating_mul(1000) / u64::from(rate));
        }
        let decoder = symphonia::default::get_codecs()
            .make(params, &DecoderOptions::default())
            .map_err(|_| TranscriptionError::AudioUndecodable)?;
        decoders.push((track.id, decoder, Resampler::new(rate, SAMPLE_RATE_HZ)));
    }
    if decoders.is_empty() {
        return Err(TranscriptionError::AudioUndecodable);
    }
    if known_ms > max_duration_ms {
        return Err(TranscriptionError::FileTooLarge);
    }

    // C3: decode packet by packet; the decoded sound is never written to disk.
    let max_samples = max_duration_ms * u64::from(SAMPLE_RATE_HZ) / 1000;
    let interval = Duration::from_millis(PROGRESS_INTERVAL_MS);
    let mut last_progress = Instant::now();
    let mut mono: Vec<f32> = Vec::new();
    loop {
        if cancel.is_cancelled() {
            return Err(TranscriptionError::Cancelled);
        }
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(AudioError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(AudioError::ResetRequired) => break,
            Err(_) => return Err(TranscriptionError::AudioUndecodable),
        };
        let Some((_, decoder, resampler)) = decoders
            .iter_mut()
            .find(|(id, _, _)| *id == packet.track_id())
        else {
            continue;
        };
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet is skipped, as players do.
            Err(AudioError::DecodeError(_)) | Err(AudioError::IoError(_)) => continue,
            Err(_) => return Err(TranscriptionError::AudioUndecodable),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        mono.clear();
        mono.extend(
            buffer
                .samples()
                .chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        resampler.push(&mono);
        if resampler.out.len() as u64 > max_samples {
            return Err(TranscriptionError::FileTooLarge);
        }
        if last_progress.elapsed() >= interval {
            last_progress = Instant::now();
            let done_ms = samples_to_ms(
                decoders
                    .iter()
                    .map(|(_, _, r)| r.out.len())
                    .max()
                    .unwrap_or(0),
            );
            progress(done_ms, known_ms.max(done_ms));
        }
    }

    let mut decoded: Vec<Vec<f32>> = decoders
        .into_iter()
        .map(|(_, _, resampler)| resampler.finish())
        .collect();
    let duration_ms = samples_to_ms(decoded.iter().map(Vec::len).max().unwrap_or(0));
    progress(duration_ms, duration_ms);

    // A recording with one audio track is treated as Mixed.
    let audio = if tracks == SpeakerTracks::YouAndOthers && decoded.len() >= 2 {
        let others = decoded.swap_remove(1);
        let you = decoded.swap_remove(0);
        vec![
            TrackAudio {
                speaker: Some("You"),
                samples: you,
            },
            TrackAudio {
                speaker: Some("Others"),
                samples: others,
            },
        ]
    } else {
        vec![TrackAudio {
            speaker: None,
            samples: mix(decoded),
        }]
    };
    Ok((audio, duration_ms))
}

fn samples_to_ms(samples: usize) -> u64 {
    samples as u64 * 1000 / u64::from(SAMPLE_RATE_HZ)
}

/// Every track's samples averaged into one.
fn mix(mut tracks: Vec<Vec<f32>>) -> Vec<f32> {
    if tracks.len() <= 1 {
        return tracks.pop().unwrap_or_default();
    }
    let count = tracks.len() as f32;
    let length = tracks.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = vec![0.0f32; length];
    for track in &tracks {
        for (o, s) in out.iter_mut().zip(track) {
            *o += *s;
        }
    }
    for o in &mut out {
        *o /= count;
    }
    out
}

/// Zero crossings of the low-pass kernel on each side.
const ZERO_CROSSINGS: f64 = 8.0;
/// Kernel table entries per input sample.
const TABLE_STEPS: usize = 256;

/// A streaming windowed-sinc (Blackman) resampler of one mono signal to a fixed rate. The input
/// is kept only as far back as the kernel reaches, so a long recording is never held at its
/// original rate.
pub(crate) struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Kernel reach in input samples on each side.
    half: f64,
    /// The kernel from 0 to `half` input samples, `TABLE_STEPS` entries per sample.
    table: Vec<f32>,
    /// Input not yet consumed; `buf[0]` is input sample `offset`.
    buf: Vec<f32>,
    /// Position of the next output, in samples from `buf[0]`.
    pos: f64,
    passthrough: bool,
    pub(crate) out: Vec<f32>,
}

impl Resampler {
    pub(crate) fn new(from_hz: u32, to_hz: u32) -> Resampler {
        let step = f64::from(from_hz) / f64::from(to_hz);
        // Below the lower of the two Nyquist frequencies.
        let cutoff = (1.0 / step).min(1.0);
        let half = ZERO_CROSSINGS / cutoff;
        let entries = (half * TABLE_STEPS as f64).ceil() as usize + 2;
        let table = (0..entries)
            .map(|i| {
                let t = i as f64 / TABLE_STEPS as f64;
                if t >= half {
                    return 0.0;
                }
                let x = t * cutoff;
                let sinc = if x == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                };
                // Blackman window over [-half, half], evaluated at t.
                let w = 0.5 + t / (2.0 * half);
                let window = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * w).cos()
                    + 0.08 * (4.0 * std::f64::consts::PI * w).cos();
                (cutoff * sinc * window) as f32
            })
            .collect();
        Resampler {
            step,
            half,
            table,
            buf: Vec::new(),
            pos: 0.0,
            passthrough: from_hz == to_hz,
            out: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, input: &[f32]) {
        if self.passthrough {
            self.out.extend_from_slice(input);
            return;
        }
        self.buf.extend_from_slice(input);
        self.produce(false);
    }

    /// The remaining outputs, treating input past the end as silence.
    pub(crate) fn finish(mut self) -> Vec<f32> {
        if !self.passthrough {
            self.produce(true);
        }
        self.out
    }

    fn produce(&mut self, last: bool) {
        let len = self.buf.len() as f64;
        loop {
            let ready = if last {
                self.pos < len
            } else {
                self.pos + self.half < len
            };
            if !ready {
                break;
            }
            let sample = self.sample_at(self.pos);
            self.out.push(sample);
            self.pos += self.step;
        }
        // Drop input the kernel can no longer reach.
        let keep_from = (self.pos - self.half).floor().max(0.0) as usize;
        let keep_from = keep_from.min(self.buf.len());
        if keep_from > 0 {
            self.buf.drain(..keep_from);
            self.pos -= keep_from as f64;
        }
    }

    fn sample_at(&self, pos: f64) -> f32 {
        let first = (pos - self.half).ceil().max(0.0) as usize;
        let last = ((pos + self.half).floor() as usize).min(self.buf.len().saturating_sub(1));
        let mut sum = 0.0f32;
        for k in first..=last {
            let t = (pos - k as f64).abs() * TABLE_STEPS as f64;
            let index = t as usize;
            let frac = (t - index as f64) as f32;
            let (Some(a), Some(b)) = (self.table.get(index), self.table.get(index + 1)) else {
                continue;
            };
            sum += self.buf[k] * (a + (b - a) * frac);
        }
        sum
    }
}
