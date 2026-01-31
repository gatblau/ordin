//! The Transcription Service reading transcript files (TranscriptionService.md Internal Logic A):
//! the Gherkin scenarios of this half, one test each named after it, and the Error Table rows a
//! transcript file can reach. Runs through the public service over the committed fixtures.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ordin_dictation::transcription::{
    Transcript, TranscriptSourceKind, TranscriptionError, TranscriptionService,
    MAX_TRANSCRIPT_FILE_BYTES,
};
use ordin_localisation::{Localisation, LocalisationConfig, PreferenceStore};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_types::time::Timestamp;
use ordin_types::CodedError;

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
        let dir = std::env::temp_dir().join(format!("ordin-trx-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn service(dir: &Dir) -> Arc<TranscriptionService> {
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
    let localisation: Arc<Localisation> = Localisation::start(
        LocalisationConfig {
            store: PreferenceStore::None,
            os_locales: vec!["en-GB".to_owned()],
            app_version: "1.0.0".to_owned(),
        },
        profile,
    )
    .await
    .expect("localisation");
    TranscriptionService::new(
        dir.0.join("speech-models"),
        localisation,
        reqwest::Client::new(),
    )
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn speakers_and_text(transcript: &Transcript) -> Vec<(Option<String>, String)> {
    transcript
        .utterances
        .iter()
        .map(|u| (u.speaker.clone(), u.text.clone()))
        .collect()
}

// Scenario: Happy path — a WebVTT transcript keeps its speakers
#[tokio::test(flavor = "multi_thread")]
async fn happy_path_a_webvtt_transcript_keeps_its_speakers() {
    let dir = Dir::new();
    let service = service(&dir).await;
    let transcript = service
        .read_transcript(&fixture("meeting.vtt"))
        .await
        .unwrap();
    assert_eq!(transcript.format, 1);
    assert_eq!(transcript.source_kind, TranscriptSourceKind::TranscriptFile);
    assert_eq!(transcript.source_name, "meeting.vtt");
    assert_eq!(transcript.language, None);
    assert_eq!(transcript.utterances.len(), 4);
    let speakers: Vec<&str> = transcript
        .utterances
        .iter()
        .map(|u| u.speaker.as_deref().expect("every utterance has a speaker"))
        .collect();
    assert_eq!(speakers, ["Sam Lee", "Priya Shah", "Sam Lee", "Priya Shah"]);
    let first = &transcript.utterances[0];
    assert_eq!((first.start_ms, first.end_ms), (1_000, 4_200));
    assert_eq!(
        first.text,
        "Thanks for joining. First item is the supplier renewal & terms."
    );
    assert_eq!(
        transcript.utterances[1].text,
        "We agreed to go with Northwind for another year."
    );
    assert_eq!(
        transcript.utterances[3].text,
        "One question is still open: who signs it?"
    );
    assert_eq!(transcript.duration_ms, 16_800);
    // The models folder is not created by reading a transcript.
    assert!(!dir.0.join("speech-models").exists());
}

// Scenario: Edge — the same meeting as .vtt, .docx and .txt
#[tokio::test(flavor = "multi_thread")]
async fn edge_the_same_meeting_as_vtt_docx_and_txt() {
    let dir = Dir::new();
    let service = service(&dir).await;
    let vtt = service
        .read_transcript(&fixture("meeting.vtt"))
        .await
        .unwrap();
    let docx = service
        .read_transcript(&fixture("meeting.docx"))
        .await
        .unwrap();
    let txt = service
        .read_transcript(&fixture("meeting.txt"))
        .await
        .unwrap();
    assert_eq!(speakers_and_text(&vtt), speakers_and_text(&docx));
    assert_eq!(speakers_and_text(&vtt), speakers_and_text(&txt));
    assert_eq!(docx.source_name, "meeting.docx");
    let starts = |t: &Transcript| {
        t.utterances
            .iter()
            .map(|u| u.start_ms / 1_000)
            .collect::<Vec<_>>()
    };
    assert_eq!(starts(&vtt), starts(&docx));
    assert_eq!(starts(&vtt), starts(&txt));
}

// Scenario: Error — an unsupported file
#[tokio::test(flavor = "multi_thread")]
async fn error_an_unsupported_file() {
    let dir = Dir::new();
    let service = service(&dir).await;
    let pdf = dir.file("notes.pdf", b"%PDF-1.7");
    let error = service.read_transcript(&pdf).await.unwrap_err();
    assert!(matches!(error, TranscriptionError::FormatUnsupported));
    assert_eq!(error.code(), "TRX_FORMAT_UNSUPPORTED");
    // Recordings are not transcript files either; upper-case extensions are transcript files.
    let audio = dir.file("call.m4a", b"x");
    assert_eq!(
        service.read_transcript(&audio).await.unwrap_err().code(),
        "TRX_FORMAT_UNSUPPORTED"
    );
    let upper = dir.file("CALL.TXT", b"0:01\nHello");
    assert_eq!(
        service
            .read_transcript(&upper)
            .await
            .unwrap()
            .utterances
            .len(),
        1
    );
}

// Error Table rows a transcript file reaches, one case each.
#[tokio::test(flavor = "multi_thread")]
async fn each_error_row_a_transcript_file_can_reach() {
    let dir = Dir::new();
    let service = service(&dir).await;
    let too_large = dir.0.join("long.txt");
    std::fs::File::create(&too_large)
        .unwrap()
        .set_len(MAX_TRANSCRIPT_FILE_BYTES + 1)
        .unwrap();
    let cases: Vec<(PathBuf, &str)> = vec![
        (dir.0.join("missing.vtt"), "TRX_FILE_UNREADABLE"),
        (too_large, "TRX_FILE_TOO_LARGE"),
        (
            dir.file("plain.vtt", b"not webvtt\n"),
            "TRX_TRANSCRIPT_MALFORMED",
        ),
        (
            dir.file("broken.docx", b"not a zip"),
            "TRX_TRANSCRIPT_MALFORMED",
        ),
        (
            dir.file("latin1.txt", b"caf\xe9"),
            "TRX_TRANSCRIPT_MALFORMED",
        ),
        (
            dir.file("silent.txt", b"0:01\n   \n0:05\n"),
            "TRX_TRANSCRIPT_EMPTY",
        ),
        (
            dir.file("silent.vtt", b"WEBVTT\n\nNOTE nothing\n"),
            "TRX_TRANSCRIPT_EMPTY",
        ),
    ];
    for (path, code) in cases {
        let error = service.read_transcript(&path).await.unwrap_err();
        assert_eq!(error.code(), code, "{}", path.display());
        assert!(!error.retryable(), "{code}");
        assert!(!error
            .to_string()
            .contains(path.file_name().unwrap().to_str().unwrap()));
    }
    // A file of exactly the limit is read.
    let mut exact = b"0:01\nHello\n".to_vec();
    exact.resize(MAX_TRANSCRIPT_FILE_BYTES as usize, b' ');
    let at_limit = dir.file("exact.txt", &exact);
    assert_eq!(
        service
            .read_transcript(&at_limit)
            .await
            .unwrap()
            .utterances
            .len(),
        1
    );
}
