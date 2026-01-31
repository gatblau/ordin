//! The Transcription Service turning recordings into text with a real speech model
//! (TranscriptionService.md Internal Logic C): the Gherkin scenarios that need the model, one test
//! each named after it.
//!
//! These tests need the larger model file (`ggml-medium.bin`, 1.5 GB) in the folder named by
//! `ORDIN_SPEECH_MODELS`, so they are ignored by default and run explicitly by plan 07's exit
//! criteria, inside a container started with `--network none`:
//!
//! ```text
//! ORDIN_SPEECH_MODELS=/models cargo test --release -p ordin-dictation --features transcription \
//!     --test speech -- --ignored --test-threads 1
//! ```

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_dictation::transcription::{
    SpeakerTracks, SpeechModelChoice, TranscribeProgress, Transcript, TranscriptSourceKind,
    TranscriptionService, CANCEL_DEADLINE_MS,
};
use ordin_localisation::{Localisation, LocalisationConfig, PreferenceStore};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_types::time::Timestamp;
use ordin_types::CodedError;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

fn models_folder() -> PathBuf {
    let folder = PathBuf::from(
        std::env::var("ORDIN_SPEECH_MODELS")
            .expect("ORDIN_SPEECH_MODELS must name the folder holding ggml-medium.bin"),
    );
    assert!(
        folder.join("ggml-medium.bin").is_file(),
        "ggml-medium.bin is not in {}",
        folder.display()
    );
    folder
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

/// A service whose only network client goes through a proxy that counts connections: any
/// request the service made would be counted.
struct Watched {
    service: Arc<TranscriptionService>,
    connections: Arc<AtomicUsize>,
}

async fn watched_service() -> Watched {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = connections.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            counter.fetch_add(1, Ordering::SeqCst);
            drop(stream);
        }
    });
    let http = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(proxy).unwrap())
        .build()
        .unwrap();
    Watched {
        service: TranscriptionService::new(models_folder(), localisation().await, http),
        connections,
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn progress() -> (
    watch::Sender<TranscribeProgress>,
    watch::Receiver<TranscribeProgress>,
) {
    watch::channel(TranscribeProgress::Decoding {
        done_ms: 0,
        total_ms: 0,
    })
}

async fn transcribe(
    service: &TranscriptionService,
    file: &Path,
    tracks: SpeakerTracks,
) -> Transcript {
    let (tx, _rx) = progress();
    service
        .transcribe_audio(
            file,
            TranscriptSourceKind::AudioFile,
            SpeechModelChoice::Larger,
            tracks,
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {}", file.display(), e.code()))
}

/// Lower-case words with punctuation removed; numbers written as words where the model writes
/// digits for the reference's words.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Word error rate: word-level edit distance over the reference length.
fn word_error_rate(reference: &[String], hypothesis: &[String]) -> f64 {
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, r) in reference.iter().enumerate() {
        let mut current = vec![i + 1; hypothesis.len() + 1];
        for (j, h) in hypothesis.iter().enumerate() {
            let substitution = previous[j] + usize::from(r != h);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    previous[hypothesis.len()] as f64 / reference.len() as f64
}

#[test]
fn the_word_error_rate_counts_edits() {
    let reference = words("the cat sat on the mat");
    assert_eq!(word_error_rate(&reference, &reference), 0.0);
    let hypothesis = words("The cat sat on a mat.");
    assert!((word_error_rate(&reference, &hypothesis) - 1.0 / 6.0).abs() < 1e-9);
}

// Scenario: Happy path — a recording of 60 seconds becomes text
// Scenario: Edge — no audio leaves the computer (the proxy counts no connection; the container
// has no network)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs ggml-medium.bin in ORDIN_SPEECH_MODELS; run by plan 07's exit criteria"]
async fn happy_path_a_recording_of_60_seconds_becomes_text() {
    let watched = watched_service().await;
    let reference = words(&std::fs::read_to_string(fixture("speech-60s.txt")).unwrap());
    for name in ["speech-60s.m4a", "speech-60s.mp3", "speech-60s.wav"] {
        let started = Instant::now();
        let transcript = transcribe(&watched.service, &fixture(name), SpeakerTracks::Mixed).await;
        assert_eq!(transcript.source_kind, TranscriptSourceKind::AudioFile);
        assert_eq!(transcript.source_name, name);
        assert_eq!(transcript.language.as_deref(), Some("en"));
        assert!(transcript.utterances.iter().all(|u| u.speaker.is_none()));
        let text: Vec<String> = transcript
            .utterances
            .iter()
            .flat_map(|u| words(&u.text))
            .collect();
        let rate = word_error_rate(&reference, &text);
        println!(
            "{name}: word error rate {:.1} % in {:.1} s",
            rate * 100.0,
            started.elapsed().as_secs_f64()
        );
        assert!(
            rate <= 0.20,
            "{name}: word error rate {:.1} %",
            rate * 100.0
        );
    }
    assert_eq!(watched.connections.load(Ordering::SeqCst), 0);
}

// Scenario: Edge — an Ordin recording is labelled You and Others
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs ggml-medium.bin in ORDIN_SPEECH_MODELS; run by plan 07's exit criteria"]
async fn edge_an_ordin_recording_is_labelled_you_and_others() {
    let watched = watched_service().await;
    let transcript = transcribe(
        &watched.service,
        &fixture("two-track.mkv"),
        SpeakerTracks::YouAndOthers,
    )
    .await;
    let said_by = |speaker: &str| -> Vec<String> {
        transcript
            .utterances
            .iter()
            .filter(|u| u.speaker.as_deref() == Some(speaker))
            .flat_map(|u| words(&u.text))
            .collect()
    };
    let you = said_by("You");
    let others = said_by("Others");
    assert!(you.contains(&"supplier".to_owned()), "You: {you:?}");
    assert!(you.contains(&"renewal".to_owned()), "You: {you:?}");
    assert!(others.contains(&"agreed".to_owned()), "Others: {others:?}");
    assert!(others.contains(&"year".to_owned()), "Others: {others:?}");
    // Merged by start time: You speaks first, Others from 6.5 s.
    let starts: Vec<u64> = transcript.utterances.iter().map(|u| u.start_ms).collect();
    assert!(starts.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(transcript.utterances[0].speaker.as_deref(), Some("You"));
    let first_others = transcript
        .utterances
        .iter()
        .find(|u| u.speaker.as_deref() == Some("Others"))
        .unwrap();
    assert!(first_others.start_ms >= 5_000, "{}", first_others.start_ms);
    assert_eq!(watched.connections.load(Ordering::SeqCst), 0);
}

/// A 60-minute WAV made of the 60-second fixture repeated, written to a temporary folder.
fn sixty_minute_recording(folder: &Path) -> PathBuf {
    let fixture = std::fs::read(fixture("speech-60s.wav")).unwrap();
    // The fixture is a plain 44-byte header followed by its samples.
    assert_eq!(&fixture[36..40], b"data");
    let data = &fixture[44..];
    let total = data.len() * 60;
    let mut wav = Vec::with_capacity(44 + total);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&u32::try_from(36 + total).unwrap().to_le_bytes());
    wav.extend_from_slice(&fixture[8..40]);
    wav.extend_from_slice(&u32::try_from(total).unwrap().to_le_bytes());
    for _ in 0..60 {
        wav.extend_from_slice(data);
    }
    let path = folder.join("meeting-60min.wav");
    std::fs::write(&path, wav).unwrap();
    path
}

// Scenario: Edge — cancel stops within five seconds
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs ggml-medium.bin in ORDIN_SPEECH_MODELS; run by plan 07's exit criteria"]
async fn edge_cancel_stops_within_five_seconds() {
    let folder = std::env::temp_dir().join(format!("ordin-trx-cancel-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let recording = sixty_minute_recording(&folder);
    let watched = watched_service().await;
    let (tx, mut rx) = progress();
    let cancel = CancellationToken::new();
    let task = {
        let service = watched.service.clone();
        let cancel = cancel.clone();
        let recording = recording.clone();
        tokio::spawn(async move {
            service
                .transcribe_audio(
                    &recording,
                    TranscriptSourceKind::Recording,
                    SpeechModelChoice::Larger,
                    SpeakerTracks::Mixed,
                    tx,
                    cancel,
                )
                .await
        })
    };
    // Cancel once recognition has started, the longest stage.
    tokio::time::timeout(Duration::from_secs(600), async {
        loop {
            rx.changed().await.expect("progress");
            if matches!(*rx.borrow(), TranscribeProgress::Recognising { .. }) {
                break;
            }
        }
    })
    .await
    .expect("recognition started");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let cancelled_at = Instant::now();
    cancel.cancel();
    let result = task.await.unwrap();
    let took = cancelled_at.elapsed();
    let _ = std::fs::remove_dir_all(&folder);
    println!("cancel returned after {} ms", took.as_millis());
    let error = result.unwrap_err();
    assert_eq!(error.code(), "TRX_CANCELLED");
    assert!(
        took <= Duration::from_millis(CANCEL_DEADLINE_MS),
        "took {} ms",
        took.as_millis()
    );
}

// C-110 (plan 07 Phase 9): how long a 60-minute recording takes with the larger model on this
// machine. Printed for the plan's trail; the budget checked is SA-MIN-03's for the machine named by
// `ORDIN_REFERENCE_MACHINE` (`apple` 20 minutes, `x86` 45 minutes). Ignored: it runs for up to an hour.
// spec: ordin-desktop/phase-1-analysis#SA-MIN-03
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs ggml-medium.bin in ORDIN_SPEECH_MODELS and ORDIN_REFERENCE_MACHINE; runs up to an hour"]
async fn timing_a_60_minute_recording_with_the_larger_model() {
    let budget_minutes = match std::env::var("ORDIN_REFERENCE_MACHINE").as_deref() {
        Ok("apple") => 20,
        Ok("x86") => 45,
        other => panic!("ORDIN_REFERENCE_MACHINE must be apple or x86, not {other:?}"),
    };
    let folder = std::env::temp_dir().join(format!("ordin-trx-timing-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let recording = sixty_minute_recording(&folder);
    let watched = watched_service().await;
    let started = Instant::now();
    let transcript = transcribe(&watched.service, &recording, SpeakerTracks::Mixed).await;
    let minutes = started.elapsed().as_secs_f64() / 60.0;
    let _ = std::fs::remove_dir_all(&folder);
    println!(
        "60-minute recording, larger model: {minutes:.1} minutes, {} utterances (budget {budget_minutes})",
        transcript.utterances.len()
    );
    assert!(!transcript.utterances.is_empty());
    assert!(
        minutes <= f64::from(budget_minutes),
        "{minutes:.1} minutes is over SA-MIN-03's {budget_minutes}"
    );
}
