//! Unit tests of the model list and download (TranscriptionService.md Internal Logic B), audio
//! decoding and the resampler (C2–C3). The model host is wiremock, except the resume test, whose
//! connection must drop half-way through a body — wiremock cannot do that, so a small TCP server
//! written here plays the host.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_localisation::{Localisation, LocalisationConfig, PreferenceStore};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_types::time::Timestamp;
use ordin_types::CodedError;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::audio::{decode, Resampler, SAMPLE_RATE_HZ};
use super::models::{free_bytes, parse_list, DownloadTuning, ModelEntry, BUNDLED_LIST};
use super::{
    DownloadProgress, SpeakerTracks, SpeechModelChoice, TranscribeProgress, TranscriptSourceKind,
    TranscriptionError, TranscriptionService, MAX_AUDIO_DURATION_MS,
};

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// A folder removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        let dir = std::env::temp_dir().join(format!("ordin-trx-model-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn models(&self) -> PathBuf {
        self.0.join("speech-models")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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

fn fast() -> DownloadTuning {
    DownloadTuning {
        read_timeout: Duration::from_millis(500),
        backoff: [Duration::from_millis(10); 3],
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A model list of one larger model served at `url` with `body`'s checksum (or `sha256`).
fn entries(url: String, sha256: String) -> Vec<ModelEntry> {
    vec![
        ModelEntry {
            choice: SpeechModelChoice::Larger,
            file_name: "ggml-test.bin".to_owned(),
            size_mb: 1,
            sha256,
            url,
        },
        ModelEntry {
            choice: SpeechModelChoice::Smaller,
            file_name: "ggml-test-small.bin".to_owned(),
            size_mb: 1,
            sha256: "0".repeat(64),
            url: "https://127.0.0.1:9/never".to_owned(),
        },
    ]
}

async fn service(dir: &Dir, models: Vec<ModelEntry>) -> Arc<TranscriptionService> {
    TranscriptionService::with_models(
        dir.models(),
        localisation().await,
        reqwest::Client::new(),
        models,
        fast(),
    )
}

/// 2.5 MB of varied bytes, so progress passes two megabyte marks.
fn model_body() -> Vec<u8> {
    (0..2_500_000u32).map(|i| (i % 251) as u8).collect()
}

fn files_in(folder: &Path) -> Vec<String> {
    std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn progress() -> (
    watch::Sender<DownloadProgress>,
    watch::Receiver<DownloadProgress>,
) {
    watch::channel(DownloadProgress {
        received_mb: 0,
        total_mb: 0,
    })
}

// ---- the bundled list

#[test]
fn the_bundled_model_list_parses_and_names_both_models() {
    let list = parse_list(BUNDLED_LIST).expect("speech-models.json must parse");
    assert_eq!(list.len(), 2);
    let larger = list
        .iter()
        .find(|m| m.choice == SpeechModelChoice::Larger)
        .unwrap();
    assert_eq!(larger.file_name, "ggml-medium.bin");
    assert_eq!(larger.size_mb, 1533);
    let smaller = list
        .iter()
        .find(|m| m.choice == SpeechModelChoice::Smaller)
        .unwrap();
    assert_eq!(smaller.file_name, "ggml-small.bin");
    assert_eq!(smaller.size_mb, 488);
}

#[test]
fn a_model_list_is_refused_when_any_value_is_out_of_bounds() {
    let good = r#"{"format":1,"models":[
      {"choice":"larger","file_name":"a.bin","size_mb":10,"sha256":"SHA","url":"URL"},
      {"choice":"smaller","file_name":"b.bin","size_mb":10,"sha256":"0000000000000000000000000000000000000000000000000000000000000000","url":"https://h/b"}]}"#;
    let hex = "a".repeat(64);
    let ok = good.replace("SHA", &hex).replace("URL", "https://h/a");
    assert!(parse_list(&ok).is_ok());
    let cases = [
        (
            "plain http",
            good.replace("SHA", &hex).replace("URL", "http://h/a"),
        ),
        (
            "upper-case hex",
            good.replace("SHA", &"A".repeat(64))
                .replace("URL", "https://h/a"),
        ),
        (
            "short hex",
            good.replace("SHA", "abc").replace("URL", "https://h/a"),
        ),
        ("format 2", ok.replace(r#""format":1"#, r#""format":2"#)),
        (
            "size 0",
            ok.replace(
                r#""size_mb":10,"sha256":"aaaa"#,
                r#""size_mb":0,"sha256":"aaaa"#,
            ),
        ),
        ("path in name", ok.replace("a.bin", "../a.bin")),
        (
            "smaller twice",
            ok.replace(r#""choice":"larger""#, r#""choice":"smaller""#),
        ),
        ("not JSON", "{".to_owned()),
    ];
    for (name, json) in cases {
        assert!(parse_list(&json).is_err(), "{name} was accepted");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn models_lists_both_with_the_folder_and_never_touches_the_network() {
    let dir = Dir::new();
    // The addresses point at a closed port: any request would fail the test by hanging or erring.
    let service = service(
        &dir,
        entries("https://127.0.0.1:9/x".to_owned(), "a".repeat(64)),
    )
    .await;
    let models = service.models().await;
    assert_eq!(models.len(), 2);
    assert!(models
        .iter()
        .all(|m| !m.present && m.folder == dir.models()));
}

// ---- download (B)

#[tokio::test(flavor = "multi_thread")]
async fn a_download_writes_the_model_only_after_its_checksum_matched() {
    let dir = Dir::new();
    let server = MockServer::start().await;
    let body = model_body();
    Mock::given(method("GET"))
        .and(path("/ggml-test.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let service = service(
        &dir,
        entries(format!("{}/ggml-test.bin", server.uri()), sha256_hex(&body)),
    )
    .await;
    let (tx, rx) = progress();
    let info = service
        .download_model(SpeechModelChoice::Larger, tx, CancellationToken::new())
        .await
        .expect("download");
    assert!(info.present);
    assert_eq!(
        std::fs::read(dir.models().join("ggml-test.bin")).unwrap(),
        body
    );
    assert_eq!(files_in(&dir.models()), vec!["ggml-test.bin".to_owned()]);
    assert_eq!(
        *rx.borrow(),
        DownloadProgress {
            received_mb: 2,
            total_mb: 1
        }
    );
    // B1: a present model is returned without another request (`expect(1)` above).
    let (tx, _rx) = progress();
    let again = service
        .download_model(SpeechModelChoice::Larger, tx, CancellationToken::new())
        .await
        .expect("present");
    assert!(again.present);
}

// Scenario: Error — damaged download
#[tokio::test(flavor = "multi_thread")]
async fn error_damaged_download() {
    let dir = Dir::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(model_body()))
        .mount(&server)
        .await;
    let service = service(
        &dir,
        entries(format!("{}/ggml-test.bin", server.uri()), "b".repeat(64)),
    )
    .await;
    let (tx, _rx) = progress();
    let error = service
        .download_model(SpeechModelChoice::Larger, tx, CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "TRX_MODEL_CHECKSUM");
    assert!(error.retryable());
    assert!(
        files_in(&dir.models()).is_empty(),
        "left {:?}",
        files_in(&dir.models())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_failing_four_times_is_download_failed_and_leaves_nothing() {
    let dir = Dir::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .expect(4)
        .mount(&server)
        .await;
    let service = service(
        &dir,
        entries(format!("{}/ggml-test.bin", server.uri()), "b".repeat(64)),
    )
    .await;
    let (tx, _rx) = progress();
    let error = service
        .download_model(SpeechModelChoice::Larger, tx, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, TranscriptionError::DownloadFailed));
    assert_eq!(error.code(), "TRX_MODEL_DOWNLOAD_FAILED");
    assert!(files_in(&dir.models()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_read_is_retried_and_then_fails() {
    let dir = Dir::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .expect(4)
        .mount(&server)
        .await;
    let service = service(
        &dir,
        entries(format!("{}/ggml-test.bin", server.uri()), "b".repeat(64)),
    )
    .await;
    let (tx, _rx) = progress();
    let error = service
        .download_model(SpeechModelChoice::Larger, tx, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, TranscriptionError::DownloadFailed));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_download_removes_the_part_file() {
    let dir = Dir::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;
    let mut slow = fast();
    slow.read_timeout = Duration::from_secs(60);
    let service = TranscriptionService::with_models(
        dir.models(),
        localisation().await,
        reqwest::Client::new(),
        entries(format!("{}/ggml-test.bin", server.uri()), "b".repeat(64)),
        slow,
    );
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        trigger.cancel();
    });
    let started = Instant::now();
    let (tx, _rx) = progress();
    let error = service
        .download_model(SpeechModelChoice::Larger, tx, cancel)
        .await
        .unwrap_err();
    assert!(matches!(error, TranscriptionError::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(files_in(&dir.models()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_download_while_one_runs_is_busy() {
    let dir = Dir::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;
    let mut slow = fast();
    slow.read_timeout = Duration::from_secs(60);
    let service = TranscriptionService::with_models(
        dir.models(),
        localisation().await,
        reqwest::Client::new(),
        entries(format!("{}/ggml-test.bin", server.uri()), "b".repeat(64)),
        slow,
    );
    let cancel = CancellationToken::new();
    let first = {
        let service = service.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let (tx, _rx) = progress();
            service
                .download_model(SpeechModelChoice::Larger, tx, cancel)
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (tx, _rx) = progress();
    let error = service
        .download_model(SpeechModelChoice::Larger, tx, CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "TRX_BUSY");
    cancel.cancel();
    assert!(matches!(
        first.await.unwrap(),
        Err(TranscriptionError::Cancelled)
    ));
}

/// A host that sends half the body then closes the connection, and answers the next request's
/// `Range` with the rest (206). Returns its address and the `Range` header it saw.
async fn dropping_host(body: Vec<u8>) -> (String, tokio::task::JoinHandle<Option<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}/ggml-test.bin", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let half = body.len() / 2;
        // First request: full length announced, half sent, connection dropped.
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\n\r\n",
            body.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&body[..half]).await.unwrap();
        socket.flush().await.unwrap();
        drop(socket);
        // Second request: must resume.
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        let range = request
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("range:"))
            .map(|l| l["range:".len()..].trim().to_owned());
        let start: usize = range
            .as_deref()
            .and_then(|r| r.strip_prefix("bytes="))
            .and_then(|r| r.strip_suffix('-'))
            .and_then(|r| r.parse().ok())
            .unwrap_or(0);
        let rest = &body[start..];
        let head = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\n\r\n",
            rest.len(),
            start,
            body.len() - 1,
            body.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(rest).await.unwrap();
        socket.flush().await.unwrap();
        range
    });
    (address, task)
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut buffer).await.unwrap();
        if n == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..n]);
    }
    String::from_utf8_lossy(&request).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_download_resumes_with_range_and_completes() {
    let dir = Dir::new();
    let body = model_body();
    let (address, host) = dropping_host(body.clone()).await;
    // `entries` requires https for the bundled list only; tests build entries directly.
    let service = service(&dir, entries(address, sha256_hex(&body))).await;
    let (tx, _rx) = progress();
    let info = service
        .download_model(SpeechModelChoice::Larger, tx, CancellationToken::new())
        .await
        .expect("resumed download");
    assert!(info.present);
    assert_eq!(
        std::fs::read(dir.models().join("ggml-test.bin")).unwrap(),
        body
    );
    assert_eq!(
        host.await.unwrap(),
        Some(format!("bytes={}-", body.len() / 2))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn remove_model_deletes_the_file_and_is_ok_when_absent() {
    let dir = Dir::new();
    let service = service(
        &dir,
        entries("https://127.0.0.1:9/x".to_owned(), "a".repeat(64)),
    )
    .await;
    service
        .remove_model(SpeechModelChoice::Larger)
        .await
        .expect("absent is ok");
    std::fs::create_dir_all(dir.models()).unwrap();
    std::fs::write(dir.models().join("ggml-test.bin"), b"model").unwrap();
    service
        .remove_model(SpeechModelChoice::Larger)
        .await
        .expect("removed");
    assert!(files_in(&dir.models()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_part_file_left_by_a_crash_is_deleted_at_start() {
    let dir = Dir::new();
    std::fs::create_dir_all(dir.models()).unwrap();
    std::fs::write(dir.models().join("ggml-test.bin.part"), b"half").unwrap();
    std::fs::write(dir.models().join("ggml-test.bin"), b"model").unwrap();
    let _service = service(
        &dir,
        entries("https://127.0.0.1:9/x".to_owned(), "a".repeat(64)),
    )
    .await;
    assert_eq!(files_in(&dir.models()), vec!["ggml-test.bin".to_owned()]);
}

#[test]
fn free_space_is_readable_for_the_temporary_folder() {
    assert!(free_bytes(&std::env::temp_dir()).is_some_and(|b| b > 0));
}

// ---- transcribe_audio preconditions (C1) that need no model

// Scenario: Error — no model yet
#[tokio::test(flavor = "multi_thread")]
async fn error_no_model_yet() {
    let dir = Dir::new();
    let service = service(
        &dir,
        entries("https://127.0.0.1:9/x".to_owned(), "a".repeat(64)),
    )
    .await;
    let (tx, _rx) = watch::channel(TranscribeProgress::Decoding {
        done_ms: 0,
        total_ms: 0,
    });
    let error = service
        .transcribe_audio(
            &fixture("speech-60s.wav"),
            TranscriptSourceKind::AudioFile,
            SpeechModelChoice::Larger,
            SpeakerTracks::Mixed,
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "TRX_MODEL_MISSING");
    assert!(files_in(&dir.models()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn transcribe_audio_refuses_other_types_and_missing_files() {
    let dir = Dir::new();
    let service = service(
        &dir,
        entries("https://127.0.0.1:9/x".to_owned(), "a".repeat(64)),
    )
    .await;
    let cases = [
        (fixture("meeting.vtt"), "TRX_FORMAT_UNSUPPORTED"),
        (dir.0.join("notes.pdf"), "TRX_FORMAT_UNSUPPORTED"),
        (dir.0.join("gone.m4a"), "TRX_FILE_UNREADABLE"),
    ];
    for (file, code) in cases {
        let (tx, _rx) = watch::channel(TranscribeProgress::Decoding {
            done_ms: 0,
            total_ms: 0,
        });
        let error = service
            .transcribe_audio(
                &file,
                TranscriptSourceKind::AudioFile,
                SpeechModelChoice::Larger,
                SpeakerTracks::Mixed,
                tx,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), code, "{}", file.display());
    }
}

// ---- decoding (C2–C3)

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn decode_fixture(
    name: &str,
    tracks: SpeakerTracks,
) -> Result<(Vec<super::audio::TrackAudio>, u64), TranscriptionError> {
    decode(
        &fixture(name),
        tracks,
        MAX_AUDIO_DURATION_MS,
        &CancellationToken::new(),
        &|_, _| {},
    )
}

#[test]
fn the_three_speech_fixtures_decode_to_sixty_seconds_at_16_khz() {
    for name in ["speech-60s.wav", "speech-60s.m4a", "speech-60s.mp3"] {
        let (audio, duration_ms) = decode_fixture(name, SpeakerTracks::Mixed).expect(name);
        assert_eq!(audio.len(), 1, "{name}");
        assert_eq!(audio[0].speaker, None);
        // Encoders add up to a few tens of milliseconds of priming.
        assert!(
            (59_900..=60_200).contains(&duration_ms),
            "{name}: {duration_ms} ms"
        );
        let expected = 60 * SAMPLE_RATE_HZ as usize;
        let got = audio[0].samples.len();
        assert!(
            got.abs_diff(expected) < SAMPLE_RATE_HZ as usize / 5,
            "{name}: {got} samples"
        );
        let peak = audio[0].samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05, "{name}: silent ({peak})");
    }
}

#[test]
fn an_ordin_recording_splits_into_you_and_others_or_mixes() {
    let (split, duration_ms) =
        decode_fixture("two-track.mkv", SpeakerTracks::YouAndOthers).expect("two tracks");
    assert_eq!(
        split.iter().map(|t| t.speaker).collect::<Vec<_>>(),
        vec![Some("You"), Some("Others")]
    );
    assert!((13_900..=14_200).contains(&duration_ms), "{duration_ms} ms");
    // Track 2 is silent for its first 6.5 s; track 1 speaks at once.
    let loud = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let first_second = SAMPLE_RATE_HZ as usize;
    assert!(loud(&split[0].samples[..first_second]) > 0.05);
    assert!(loud(&split[1].samples[..first_second]) < 0.01);
    let (mixed, _) = decode_fixture("two-track.mkv", SpeakerTracks::Mixed).expect("mixed");
    assert_eq!(mixed.len(), 1);
    assert_eq!(mixed[0].speaker, None);
}

// OBS writes a video track before the two audio tracks (plan 07 Phase 12 smoke test, OBS 33 on
// macOS); the video track is passed over and the audio reads as without it.
#[test]
fn an_obs_recording_with_a_video_track_splits_into_you_and_others() {
    let (split, duration_ms) = decode_fixture("two-track-video.mkv", SpeakerTracks::YouAndOthers)
        .expect("video and two audio tracks");
    assert_eq!(
        split.iter().map(|t| t.speaker).collect::<Vec<_>>(),
        vec![Some("You"), Some("Others")]
    );
    assert!((13_900..=14_200).contains(&duration_ms), "{duration_ms} ms");
}

#[test]
fn a_single_track_recording_asked_as_you_and_others_is_mixed() {
    let (audio, _) = decode_fixture("speech-60s.m4a", SpeakerTracks::YouAndOthers).expect("m4a");
    assert_eq!(audio.len(), 1);
    assert_eq!(audio[0].speaker, None);
}

#[test]
fn decoding_refuses_long_recordings_damaged_files_and_stops_on_cancel() {
    let too_long = decode(
        &fixture("speech-60s.mp3"),
        SpeakerTracks::Mixed,
        10_000,
        &CancellationToken::new(),
        &|_, _| {},
    );
    assert!(matches!(too_long, Err(TranscriptionError::FileTooLarge)));

    let dir = Dir::new();
    let damaged = dir.0.join("noise.wav");
    std::fs::write(&damaged, vec![0x5au8; 4096]).unwrap();
    let undecodable = decode(
        &damaged,
        SpeakerTracks::Mixed,
        MAX_AUDIO_DURATION_MS,
        &CancellationToken::new(),
        &|_, _| {},
    );
    assert!(matches!(
        undecodable,
        Err(TranscriptionError::AudioUndecodable)
    ));

    let cancel = CancellationToken::new();
    cancel.cancel();
    let cancelled = decode(
        &fixture("speech-60s.wav"),
        SpeakerTracks::Mixed,
        MAX_AUDIO_DURATION_MS,
        &cancel,
        &|_, _| {},
    );
    assert!(matches!(cancelled, Err(TranscriptionError::Cancelled)));
}

#[test]
fn decoding_reports_progress_ending_at_the_full_duration() {
    let last = std::sync::Mutex::new((0u64, 0u64));
    decode(
        &fixture("speech-60s.m4a"),
        SpeakerTracks::Mixed,
        MAX_AUDIO_DURATION_MS,
        &CancellationToken::new(),
        &|done, total| *last.lock().unwrap() = (done, total),
    )
    .expect("m4a");
    let (done, total) = *last.lock().unwrap();
    assert_eq!(done, total);
    assert!(done >= 59_900);
}

// ---- the resampler

fn sine(rate: u32, hz: f64, seconds: f64) -> Vec<f32> {
    let n = (f64::from(rate) * seconds) as usize;
    (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate)).sin() as f32 * 0.5)
        .collect()
}

/// Amplitude of `hz` in `samples` at `rate`, by correlation.
fn amplitude(samples: &[f32], rate: u32, hz: f64) -> f64 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, s) in samples.iter().enumerate() {
        let phase = 2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate);
        re += f64::from(*s) * phase.cos();
        im += f64::from(*s) * phase.sin();
    }
    2.0 * (re * re + im * im).sqrt() / samples.len() as f64
}

#[test]
fn the_resampler_keeps_speech_frequencies_and_removes_those_above_8_khz() {
    for from in [44_100u32, 48_000, 22_050, 8_000] {
        let mut resampler = Resampler::new(from, SAMPLE_RATE_HZ);
        let input = sine(from, 440.0, 2.0);
        // Fed in uneven pieces, as packets arrive.
        for chunk in input.chunks(1_000 + (from as usize % 777)) {
            resampler.push(chunk);
        }
        let out = resampler.finish();
        let expected = (2.0 * f64::from(SAMPLE_RATE_HZ)) as usize;
        assert!(
            out.len().abs_diff(expected) <= 2,
            "{from}: {} samples",
            out.len()
        );
        let kept = amplitude(&out[1000..out.len() - 1000], SAMPLE_RATE_HZ, 440.0);
        assert!((kept - 0.5).abs() < 0.02, "{from}: 440 Hz at {kept}");
    }
    // 12 kHz from 48 kHz is above the new Nyquist frequency and must be removed.
    let mut resampler = Resampler::new(48_000, SAMPLE_RATE_HZ);
    resampler.push(&sine(48_000, 12_000.0, 1.0));
    let out = resampler.finish();
    let peak = out[500..out.len() - 500]
        .iter()
        .fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak < 0.02, "aliasing left {peak}");
}

#[test]
fn the_resampler_passes_16_khz_through_unchanged() {
    let input = sine(16_000, 300.0, 0.5);
    let mut resampler = Resampler::new(16_000, SAMPLE_RATE_HZ);
    resampler.push(&input);
    assert_eq!(resampler.finish(), input);
}

// ---- segment start after silence (C6)

#[test]
fn a_segment_starting_in_silence_moves_to_its_first_sound() {
    use super::recognise::first_sound_ms;
    // 2 s of silence, then 1 s of a 300 Hz tone, at 16 kHz.
    let mut samples = vec![0.0f32; 32_000];
    samples.extend(sine(16_000, 300.0, 1.0));
    assert_eq!(first_sound_ms(&samples, 0, 3_000, 0.05), 2_000);
    // A start already in the sound stays put.
    assert_eq!(first_sound_ms(&samples, 2_500, 3_000, 0.05), 2_500);
    // No sound in the range, or a range past the end: unchanged.
    assert_eq!(first_sound_ms(&samples, 0, 1_500, 0.05), 0);
    assert_eq!(first_sound_ms(&samples, 4_000, 5_000, 0.05), 4_000);
}
