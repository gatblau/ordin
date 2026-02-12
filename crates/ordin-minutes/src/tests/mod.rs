//! The Minutes Service with the real Journal and Identity on a temporary SQLite file, the real
//! Transcription Service, the OBS Recorder Adapter against the fake OBS of plan 07 Phase 4, and a
//! scripted `MinutesWriter` (MinutesService.md *Acceptance Criteria*, one test per scenario named
//! after it, and one per Error Table row).
//!
//! Tests that transcribe audio need the larger speech model, `ggml-medium.bin`, in the folder
//! named by `ORDIN_SPEECH_MODELS`, so they are ignored by default and run by plan 07's exit
//! criteria:
//!
//! ORDIN_SPEECH_MODELS=/models cargo test --release -p ordin-minutes -- --ignored --test-threads 1

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ordin_dictation::transcription::{SpeechModelChoice, TranscriptionService};
use ordin_identity::{IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry,
    ModuleMigrations, NewMinutes, SystemClock, MIGRATIONS,
};
use ordin_localisation::{Localisation, LocalisationConfig, PreferenceStore};
use ordin_obs::fake::{FakeObs, Obs};
use ordin_obs::{ObsError, ObsRecorder};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_types::id::{MinutesId, WorkspaceId};
use ordin_types::time::Timestamp;
use ordin_types::wire::minutes::MinutesSource;
use ordin_types::{CodedError, RepairAction};
use tokio_util::sync::CancellationToken;

use crate::service::render;
use crate::{
    MinutesError, MinutesJob, MinutesJobState, MinutesService, MinutesSettingsChange,
    MinutesWriter, SpeechModelSetting, WriteMinutesRequest,
};

const BODY: &str = "## Summary\nThe team reviewed the supplier renewal.\n\n## Decisions\n- Renew for twelve months.\n\n## Actions\n- Send the renewal letter — Priya Shah\n- Check the penalties clause — unassigned\n\n## Open questions\n- Should the renewal include the new warehouse?\n";

struct ProfileClock;

impl Clock for ProfileClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

async fn localisation() -> Arc<Localisation> {
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::NotUsed,
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(ProfileClock),
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

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ordin-dictation/tests/fixtures")
        .join(name)
}

/// A temporary folder removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-minutes-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Writes {
    Keep,
    Fail,
}

/// Keeps the four-heading minutes in the Journal, as Ordin's `keep_minutes` tool does.
struct ScriptedWriter {
    journal: Arc<Journal>,
    writes: Mutex<Writes>,
    calls: Mutex<Vec<WriteMinutesRequest>>,
}

#[async_trait::async_trait]
impl MinutesWriter for ScriptedWriter {
    fn writer_name(&self) -> String {
        "the AI service".to_owned()
    }

    async fn write(
        &self,
        req: WriteMinutesRequest,
        _cancel: CancellationToken,
    ) -> Result<MinutesId, MinutesError> {
        self.calls.lock().expect("calls").push(req.clone());
        if *self.writes.lock().expect("writes") == Writes::Fail {
            return Err(MinutesError::WriteFailed);
        }
        let minutes = self
            .journal
            .keep_minutes(NewMinutes {
                workspace_id: req.workspace_id,
                title: "Supplier call".to_owned(),
                source: req.source,
                source_name: req.source_name,
                body: BODY.to_owned(),
                recording: req.recording,
                run_id: None,
            })
            .await
            .map_err(|_| MinutesError::WriteFailed)?;
        Ok(minutes.id)
    }
}

struct Setup {
    notice_seen: bool,
    delete_recordings: bool,
    /// The real models folder of `ORDIN_SPEECH_MODELS`; else an empty one.
    real_models: bool,
    /// `settings.json` is written before the service starts; else there is none.
    settings_file: bool,
    /// The speech model consent was given earlier (MinutesService.md C1).
    consent_given: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            notice_seen: true,
            delete_recordings: false,
            real_models: false,
            settings_file: true,
            consent_given: false,
        }
    }
}

struct Env {
    dir: TempDir,
    ws: WorkspaceId,
    journal: Arc<Journal>,
    writer: Arc<ScriptedWriter>,
    fake: FakeObs,
    models: PathBuf,
    service: Arc<MinutesService>,
    shutdown: CancellationToken,
}

impl Drop for Env {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

impl Env {
    async fn start(setup: Setup) -> Env {
        let dir = TempDir::new();
        let data = dir.0.join("data");
        std::fs::create_dir_all(&data).expect("data folder");
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: data.join("journal.db"),
            backup_dir: data.join("backups"),
        })
        .await
        .expect("open the datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).expect("journal");
        registry
            .register(ModuleMigrations {
                module: ordin_identity::MODULE_NAME,
                sqlite: ordin_identity::SQLITE_MIGRATIONS,
                postgres: &[],
            })
            .expect("identity");
        registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .expect("apply the migrations");
        let key_file = data.join("secrets.key");
        std::fs::write(&key_file, STANDARD.encode([7u8; 32])).expect("key file");
        let identity = Arc::new(
            IdentityService::new(
                IdentityConfig {
                    program: Program::Desktop,
                    data_root: data.clone(),
                    keychain_service: "io.gatblau.ordin.test".to_owned(),
                    secret_backend: SecretBackendConfig::EncryptedFile {
                        key_source: KeySource::KeyFile(key_file),
                    },
                    github_app_client_id: None,
                    microsoft_client_id: None,
                    http: reqwest::Client::new(),
                    browser: None,
                },
                Some(store.sqlite().expect("sqlite").clone()),
            )
            .await
            .expect("identity"),
        );
        let journal = Arc::new(
            Journal::start(
                store,
                JournalOptions {
                    automatic_pruning: false,
                    prune_interval: Duration::from_secs(24 * 3600),
                    clock: Arc::new(SystemClock),
                },
            )
            .await
            .expect("journal"),
        );
        let localisation = localisation().await;

        // OBS writes into its own folder, outside Ordin's.
        let obs_folder = dir.0.join("obs");
        std::fs::create_dir_all(&obs_folder).expect("obs folder");
        let output = obs_folder.join("2026-01-14 10-02-11.mkv");
        std::fs::copy(fixture("two-track.mkv"), &output).expect("recording");
        let fake = FakeObs::start(Obs::new("30.2.0", output)).await;
        let recorder = ObsRecorder::for_fake(identity, localisation.clone());

        let models = if setup.real_models {
            PathBuf::from(
                std::env::var("ORDIN_SPEECH_MODELS")
                    .expect("ORDIN_SPEECH_MODELS must name the folder holding ggml-medium.bin"),
            )
        } else {
            let models = dir.0.join("models");
            std::fs::create_dir_all(&models).expect("models folder");
            models
        };
        let transcription =
            TranscriptionService::new(models.clone(), localisation.clone(), reqwest::Client::new());

        let folder = data.join("minutes");
        std::fs::create_dir_all(&folder).expect("minutes folder");
        let seen = setup.notice_seen.then(|| Timestamp::now().to_wire());
        let consent = setup.consent_given.then(|| Timestamp::now().to_wire());
        if setup.settings_file {
            std::fs::write(
                folder.join("settings.json"),
                serde_json::json!({
                    "format": 1,
                    "speech_model": "larger",
                    "delete_recordings_after_save": setup.delete_recordings,
                    "obs_port": fake.port,
                    "model_consent_at": consent,
                    "writer_notice_seen_at": seen,
                })
                .to_string(),
            )
            .expect("settings");
        }

        let writer = Arc::new(ScriptedWriter {
            journal: journal.clone(),
            writes: Mutex::new(Writes::Keep),
            calls: Mutex::new(Vec::new()),
        });
        let service = MinutesService::new(
            folder,
            transcription,
            recorder,
            journal.clone(),
            localisation,
            writer.clone(),
        )
        .expect("service");
        let shutdown = CancellationToken::new();
        let running = service.clone();
        let stop = shutdown.clone();
        tokio::spawn(async move { running.run(stop).await });
        Env {
            dir,
            ws: WorkspaceId::new(),
            journal,
            writer,
            fake,
            models,
            service,
            shutdown,
        }
    }

    fn minutes_folder(&self) -> PathBuf {
        self.dir.0.join("data").join("minutes")
    }

    fn workspace_folder(&self) -> PathBuf {
        self.minutes_folder().join(self.ws.to_string())
    }

    /// Waits until the job satisfies `done`; `limit` seconds at most.
    async fn until(
        &self,
        job: &MinutesJob,
        limit: u64,
        done: impl Fn(&MinutesJobState) -> bool,
    ) -> MinutesJob {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(limit);
        loop {
            let jobs = self.service.jobs(job.workspace_id).await.expect("jobs");
            let now = jobs
                .into_iter()
                .find(|j| j.id == job.id)
                .expect("the job is listed");
            if done(&now.state) {
                return now;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the job is still {:?}",
                now.state
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn until_finished(&self, job: &MinutesJob, limit: u64) -> MinutesJob {
        self.until(job, limit, MinutesJobState::finished).await
    }

    fn calls(&self) -> usize {
        self.writer.calls.lock().expect("calls").len()
    }

    async fn keep(&self) -> MinutesId {
        self.journal
            .keep_minutes(NewMinutes {
                workspace_id: self.ws,
                title: "Supplier call".to_owned(),
                source: MinutesSource::TranscriptFile,
                source_name: "meeting.vtt".to_owned(),
                body: BODY.to_owned(),
                recording: None,
                run_id: None,
            })
            .await
            .expect("keep")
            .id
    }
}

fn done_id(job: &MinutesJob) -> MinutesId {
    match &job.state {
        MinutesJobState::Done { minutes_id } => *minutes_id,
        other => panic!("the job is {other:?}"),
    }
}

fn failed_code(job: &MinutesJob) -> &str {
    match &job.state {
        MinutesJobState::Failed { code, message } => {
            assert!(!message.is_empty());
            code
        }
        other => panic!("the job is {other:?}"),
    }
}

fn files_in(folder: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(folder)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

// Scenario: Happy path — minutes from a WebVTT transcript
#[tokio::test]
async fn happy_path_minutes_from_a_webvtt_transcript() {
    let env = Env::start(Setup::default()).await;
    let job = env
        .service
        .import(env.ws, fixture("meeting.vtt"))
        .await
        .expect("import");
    assert_eq!(job.source, MinutesSource::TranscriptFile);
    assert_eq!(job.source_name, "meeting.vtt");
    let job = env.until_finished(&job, 20).await;
    let minutes = env
        .journal
        .minutes(done_id(&job))
        .await
        .expect("journal")
        .expect("kept");
    for heading in [
        "## Summary",
        "## Decisions",
        "## Actions",
        "## Open questions",
    ] {
        assert!(minutes.body.contains(heading), "{heading}");
    }
    let actions = minutes
        .body
        .split("## Actions")
        .nth(1)
        .and_then(|rest| rest.split("## ").next())
        .expect("actions");
    for line in actions.lines().filter(|l| l.starts_with("- ")) {
        let owner = line.rsplit(" — ").next().expect("owner");
        assert!(!owner.is_empty() && line.contains(" — "), "{line}");
    }
    let calls = env.writer.calls.lock().expect("calls").clone();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].transcript_text.starts_with("[00:00:"));
    // B7: the transcript is kept under the minutes' id.
    let transcript = env
        .service
        .transcript(minutes.id)
        .await
        .expect("transcript");
    assert!(!transcript.utterances.is_empty());
}

// Scenario: Edge — consent before the first audio
#[tokio::test]
async fn edge_consent_before_the_first_audio() {
    let env = Env::start(Setup::default()).await;
    let audio = env
        .service
        .import(env.ws, fixture("speech-60s.m4a"))
        .await
        .expect("import");
    assert_eq!(audio.source, MinutesSource::AudioFile);
    env.until(&audio, 20, |s| *s == MinutesJobState::WaitingForModel)
        .await;
    let status = env.service.status(env.ws).await.expect("status");
    assert!(!status.model_consent_given);
    assert!(!status.model.present);
    assert_eq!(status.model.size_mb, 1533);
    let cancelled = env.service.cancel_job(audio.id).await.expect("cancel");
    assert_eq!(cancelled.state, MinutesJobState::Cancelled);
    let transcript = env
        .service
        .import(env.ws, fixture("meeting.vtt"))
        .await
        .expect("import");
    done_id(&env.until_finished(&transcript, 20).await);
    assert!(files_in(&env.models).is_empty(), "nothing was downloaded");
}

// Scenario: Edge — the one-time notice
#[tokio::test]
async fn edge_the_one_time_notice() {
    let env = Env::start(Setup {
        notice_seen: false,
        ..Setup::default()
    })
    .await;
    let first = env
        .service
        .import(env.ws, fixture("meeting.vtt"))
        .await
        .expect("import");
    env.until(&first, 20, |s| *s == MinutesJobState::WaitingForNotice)
        .await;
    assert_eq!(env.calls(), 0);
    let status = env.service.status(env.ws).await.expect("status");
    assert!(!status.writer_notice_seen);
    assert_eq!(status.writer_name, "the AI service");
    env.service
        .accept_writer_notice()
        .await
        .expect("accept the notice");
    done_id(&env.until_finished(&first, 20).await);
    assert_eq!(env.calls(), 1);
    let second = env
        .service
        .import(env.ws, fixture("meeting.txt"))
        .await
        .expect("import");
    let second = env
        .until(&second, 20, |s| {
            assert_ne!(*s, MinutesJobState::WaitingForNotice);
            s.finished()
        })
        .await;
    done_id(&second);
    assert_eq!(env.calls(), 2);
}

// Scenario: Happy path — record, stop, minutes with no further input
#[tokio::test]
#[ignore = "needs ggml-medium.bin in ORDIN_SPEECH_MODELS; run by plan 07's exit criteria"]
async fn happy_path_record_stop_minutes_with_no_further_input() {
    let env = Env::start(Setup {
        real_models: true,
        ..Setup::default()
    })
    .await;
    let recording = env.service.record(env.ws).await.expect("record");
    assert!(recording.others_heard);
    let job = env.service.stop(recording.id).await.expect("stop");
    assert_eq!(job.source, MinutesSource::Recording);
    let moved = env
        .workspace_folder()
        .join("recordings")
        .join(format!("{}.mkv", recording.id));
    assert!(moved.is_file(), "moved into the workspace's recordings");
    assert!(!env.fake.obs().output_path.exists());
    let job = env.until_finished(&job, 600).await;
    let minutes = env
        .journal
        .minutes(done_id(&job))
        .await
        .expect("journal")
        .expect("kept");
    assert_eq!(minutes.recording, Some(recording.id));
    let text = env.writer.calls.lock().expect("calls")[0]
        .transcript_text
        .clone();
    assert!(text.contains("You: ") || text.contains("Others: "));
}

// Scenario: Edge — delete recordings after the minutes are saved
#[tokio::test]
#[ignore = "needs ggml-medium.bin in ORDIN_SPEECH_MODELS; run by plan 07's exit criteria"]
async fn edge_delete_recordings_after_the_minutes_are_saved() {
    let env = Env::start(Setup {
        real_models: true,
        delete_recordings: true,
        ..Setup::default()
    })
    .await;
    let recording = env.service.record(env.ws).await.expect("record");
    let job = env.service.stop(recording.id).await.expect("stop");
    let minutes_id = done_id(&env.until_finished(&job, 600).await);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let minutes = env.journal.minutes(minutes_id).await.expect("journal");
        if minutes.expect("kept").recording.is_none() {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(files_in(&env.workspace_folder().join("recordings")).is_empty());
    env.service
        .transcript(minutes_id)
        .await
        .expect("the transcript is kept");
}

// Scenario: Error — writing fails
#[tokio::test]
async fn error_writing_fails() {
    let env = Env::start(Setup::default()).await;
    *env.writer.writes.lock().expect("writes") = Writes::Fail;
    let job = env
        .service
        .import(env.ws, fixture("meeting.vtt"))
        .await
        .expect("import");
    let job = env.until_finished(&job, 20).await;
    assert_eq!(failed_code(&job), "MIN_WRITE_FAILED");
    let kept = env
        .workspace_folder()
        .join("transcripts")
        .join(format!("{}.json", job.id));
    assert!(kept.is_file(), "the transcript file remains");
    // B7: Try again goes on from the kept transcript.
    *env.writer.writes.lock().expect("writes") = Writes::Keep;
    let again = env.service.retry(job.id).await.expect("retry");
    assert_eq!(again.state, MinutesJobState::Queued);
    let minutes_id = done_id(&env.until_finished(&again, 20).await);
    assert!(!kept.exists());
    env.service
        .transcript(minutes_id)
        .await
        .expect("renamed to the minutes' id");
}

// Scenario: Error — save over a newer version
#[tokio::test]
async fn error_save_over_a_newer_version() {
    let env = Env::start(Setup::default()).await;
    let id = env.keep().await;
    for base in 1..=2 {
        env.service
            .save_edit(id, format!("{BODY}\nEdit {base}"), base)
            .await
            .expect("edit");
    }
    let error = env
        .service
        .save_edit(id, BODY.to_owned(), 2)
        .await
        .expect_err("stale");
    assert_eq!(error.code(), "MIN_EDIT_CONFLICT");
    let saved = env
        .service
        .save_edit(id, "## Summary\nShorter.".to_owned(), 3)
        .await
        .expect("save at the current revision");
    assert_eq!(saved.revision, 4);
}

// Scenario: Edge — an imported file is never moved or deleted
#[tokio::test]
#[ignore = "needs ggml-medium.bin in ORDIN_SPEECH_MODELS; run by plan 07's exit criteria"]
async fn edge_an_imported_file_is_never_moved_or_deleted() {
    let env = Env::start(Setup {
        real_models: true,
        delete_recordings: true,
        ..Setup::default()
    })
    .await;
    let imported = env.dir.0.join("Supplier call.m4a");
    std::fs::copy(fixture("speech-60s.m4a"), &imported).expect("copy");
    let job = env
        .service
        .import(env.ws, imported.clone())
        .await
        .expect("import");
    assert_eq!(job.source_name, "Supplier call.m4a");
    done_id(&env.until_finished(&job, 600).await);
    assert!(imported.is_file(), "still where it was");
    assert!(files_in(&env.workspace_folder().join("recordings")).is_empty());
}

// The same scenario without the speech model: a transcript file with the deletion setting on.
#[tokio::test]
async fn edge_an_imported_transcript_is_never_moved_or_deleted() {
    let env = Env::start(Setup {
        delete_recordings: true,
        ..Setup::default()
    })
    .await;
    let imported = env.dir.0.join("Supplier call.vtt");
    std::fs::copy(fixture("meeting.vtt"), &imported).expect("copy");
    let before = std::fs::read(&imported).expect("read");
    let job = env
        .service
        .import(env.ws, imported.clone())
        .await
        .expect("import");
    done_id(&env.until_finished(&job, 20).await);
    assert_eq!(std::fs::read(&imported).expect("still there"), before);
}

// D1–D3 without the model: stop moves OBS's file and queues a recording job with no other call.
#[tokio::test]
async fn record_then_stop_moves_the_file_and_queues_the_job() {
    let env = Env::start(Setup::default()).await;
    let recording = env.service.record(env.ws).await.expect("record");
    let trace: serde_json::Value = serde_json::from_slice(
        &std::fs::read(env.minutes_folder().join("recording.json")).expect("recording.json"),
    )
    .expect("json");
    assert_eq!(trace["id"], recording.id.to_string());
    assert_eq!(
        trace["record_folder"],
        env.dir.0.join("obs").to_string_lossy().as_ref()
    );
    let status = env.service.status(env.ws).await.expect("status");
    assert_eq!(status.recording.as_ref(), Some(&recording));
    let job = env.service.stop(recording.id).await.expect("stop");
    assert_eq!(job.source, MinutesSource::Recording);
    assert_eq!(job.source_name, format!("{}.mkv", recording.id));
    assert!(env
        .workspace_folder()
        .join("recordings")
        .join(&job.source_name)
        .is_file());
    assert!(!env.minutes_folder().join("recording.json").exists());
    env.until(&job, 20, |s| *s == MinutesJobState::WaitingForModel)
        .await;
    assert!(env
        .service
        .status(env.ws)
        .await
        .expect("status")
        .recording
        .is_none());
}

// Security: a path OBS names that is not a recording is never moved or deleted.
#[tokio::test]
async fn a_stop_answer_that_is_not_a_recording_is_left_in_place() {
    let env = Env::start(Setup::default()).await;
    let other = env.dir.0.join("notes.txt");
    std::fs::write(&other, b"someone's notes").expect("write");
    env.fake.obs().output_path = other.clone();
    let recording = env.service.record(env.ws).await.expect("record");
    let job = env.service.stop(recording.id).await.expect("stop");
    assert_eq!(job.source_name, "notes.txt");
    assert_eq!(
        std::fs::read(&other).expect("still there"),
        b"someone's notes"
    );
    assert!(files_in(&env.workspace_folder().join("recordings")).is_empty());
}

// D4: OBS closed during the recording → the newest `.mkv` in the folder read at start.
#[tokio::test]
async fn obs_closed_during_a_recording_finds_the_newest_recording() {
    let env = Env::start(Setup::default()).await;
    let recording = env.service.record(env.ws).await.expect("record");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let obs = env.dir.0.join("obs");
    std::fs::write(obs.join("older.mkv"), b"older").expect("older");
    tokio::time::sleep(Duration::from_millis(50)).await;
    std::fs::write(obs.join("2026-01-14 10-30-00.mkv"), b"newest").expect("newest");
    std::fs::write(obs.join("notes.txt"), b"not a recording").expect("other");
    env.fake.close();
    let job = env.service.stop(recording.id).await.expect("stop");
    let moved = env
        .workspace_folder()
        .join("recordings")
        .join(&job.source_name);
    assert_eq!(std::fs::read(moved).expect("moved"), b"newest");
}

// D4: OBS closed and no recording newer than the start → OBS_OUTPUT_MISSING.
#[tokio::test]
async fn obs_closed_with_no_recording_is_output_missing() {
    let env = Env::start(Setup::default()).await;
    let old = env.fake.obs().output_path.clone();
    let recording = {
        // The fixture was written before the recording started.
        std::fs::remove_file(&old).expect("remove");
        env.service.record(env.ws).await.expect("record")
    };
    env.fake.close();
    let error = env.service.stop(recording.id).await.expect_err("missing");
    assert_eq!(error.code(), "OBS_OUTPUT_MISSING");
    assert!(env
        .service
        .status(env.ws)
        .await
        .expect("status")
        .recording
        .is_none());
}

// D5: quit while recording stops the recording and queues its job.
#[tokio::test]
async fn shutdown_stops_a_running_recording() {
    let env = Env::start(Setup::default()).await;
    let recording = env.service.record(env.ws).await.expect("record");
    env.service.shutdown().await;
    assert!(!env.fake.obs().recording);
    let jobs = env.service.jobs(env.ws).await.expect("jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].source_name, format!("{}.mkv", recording.id));
}

// A2: at the next start, a stopped recording with no minutes is queued again.
#[tokio::test]
async fn a_recording_without_minutes_is_queued_again_at_start() {
    let env = Env::start(Setup::default()).await;
    let recordings = env.workspace_folder().join("recordings");
    std::fs::create_dir_all(&recordings).expect("recordings");
    let id = uuid::Uuid::now_v7();
    std::fs::copy(
        fixture("two-track.mkv"),
        recordings.join(format!("{id}.mkv")),
    )
    .expect("copy");
    let service = MinutesService::new(
        env.minutes_folder(),
        TranscriptionService::new(
            env.models.clone(),
            localisation().await,
            reqwest::Client::new(),
        ),
        env.service_recorder().await,
        env.journal.clone(),
        localisation().await,
        env.writer.clone(),
    )
    .expect("service");
    let shutdown = CancellationToken::new();
    let running = service.clone();
    let stop = shutdown.clone();
    tokio::spawn(async move { running.run(stop).await });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let jobs = service.jobs(env.ws).await.expect("jobs");
        if let Some(job) = jobs.first() {
            assert_eq!(job.source, MinutesSource::Recording);
            assert_eq!(job.source_name, format!("{id}.mkv"));
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "not queued");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    shutdown.cancel();
}

impl Env {
    /// A second adapter for a second service, with its own Identity (the fake asks no password).
    async fn service_recorder(&self) -> Arc<ObsRecorder> {
        let dir = self.dir.0.join("second");
        std::fs::create_dir_all(&dir).expect("folder");
        ObsRecorder::for_fake(second_identity(&dir).await, localisation().await)
    }
}

async fn second_identity(data: &Path) -> Arc<IdentityService> {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data.join("journal.db"),
        backup_dir: data.join("backups"),
    })
    .await
    .expect("open");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry
        .register(ModuleMigrations {
            module: ordin_identity::MODULE_NAME,
            sqlite: ordin_identity::SQLITE_MIGRATIONS,
            postgres: &[],
        })
        .expect("identity");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("migrate");
    let key_file = data.join("secrets.key");
    std::fs::write(&key_file, STANDARD.encode([7u8; 32])).expect("key");
    Arc::new(
        IdentityService::new(
            IdentityConfig {
                program: Program::Desktop,
                data_root: data.to_path_buf(),
                keychain_service: "io.gatblau.ordin.test".to_owned(),
                secret_backend: SecretBackendConfig::EncryptedFile {
                    key_source: KeySource::KeyFile(key_file),
                },
                github_app_client_id: None,
                microsoft_client_id: None,
                http: reqwest::Client::new(),
                browser: None,
            },
            Some(store.sqlite().expect("sqlite").clone()),
        )
        .await
        .expect("identity"),
    )
}

// Error Table: MIN_NOT_FOUND
#[tokio::test]
async fn error_table_min_not_found() {
    let env = Env::start(Setup::default()).await;
    let unknown = MinutesId::new();
    for error in [
        env.service
            .save_edit(unknown, BODY.to_owned(), 1)
            .await
            .err(),
        env.service.delete_recording(unknown).await.err(),
        env.service.remove(unknown).await.err(),
        env.service.transcript(unknown).await.err(),
    ] {
        assert_eq!(error.expect("refused").code(), "MIN_NOT_FOUND");
    }
}

// Error Table: MIN_JOB_NOT_FOUND
#[tokio::test]
async fn error_table_min_job_not_found() {
    let env = Env::start(Setup::default()).await;
    let unknown = ordin_types::id::MinutesJobId::new();
    for error in [
        env.service.cancel_job(unknown).await.err(),
        env.service.retry(unknown).await.err(),
    ] {
        assert_eq!(error.expect("refused").code(), "MIN_JOB_NOT_FOUND");
    }
}

// Error Table: MIN_JOB_FINISHED
#[tokio::test]
async fn error_table_min_job_finished() {
    let env = Env::start(Setup::default()).await;
    let job = env
        .service
        .import(env.ws, fixture("meeting.vtt"))
        .await
        .expect("import");
    let job = env.until_finished(&job, 20).await;
    for error in [
        env.service.cancel_job(job.id).await.err(),
        env.service.retry(job.id).await.err(),
    ] {
        assert_eq!(error.expect("refused").code(), "MIN_JOB_FINISHED");
    }
}

// Error Table: MIN_RECORDING_NOT_FOUND
#[tokio::test]
async fn error_table_min_recording_not_found() {
    let env = Env::start(Setup::default()).await;
    let error = env
        .service
        .stop(ordin_types::id::RecordingId::new())
        .await
        .expect_err("unknown");
    assert_eq!(error.code(), "MIN_RECORDING_NOT_FOUND");
    // Minutes without a recording have none to delete.
    let id = env.keep().await;
    let error = env.service.delete_recording(id).await.expect_err("none");
    assert_eq!(error.code(), "MIN_RECORDING_NOT_FOUND");
}

// Error Table: MIN_RECORDING_IN_PROGRESS
#[tokio::test]
async fn error_table_min_recording_in_progress() {
    let env = Env::start(Setup::default()).await;
    env.service.record(env.ws).await.expect("record");
    let error = env.service.record(env.ws).await.expect_err("second");
    assert_eq!(error.code(), "MIN_RECORDING_IN_PROGRESS");
    assert_eq!(env.fake.obs().count("StartRecord"), 1);
}

// Error Table: MIN_BODY_INVALID
#[tokio::test]
async fn error_table_min_body_invalid() {
    let env = Env::start(Setup::default()).await;
    let id = env.keep().await;
    for body in [String::new(), "x".repeat(crate::MAX_BODY_BYTES + 1)] {
        let error = env.service.save_edit(id, body, 1).await.expect_err("body");
        assert_eq!(error.code(), "MIN_BODY_INVALID");
    }
}

// Error Table: MIN_SETTING_INVALID
#[tokio::test]
async fn error_table_min_setting_invalid() {
    let env = Env::start(Setup::default()).await;
    for change in [
        MinutesSettingsChange::default(),
        MinutesSettingsChange {
            obs_port: Some(80),
            ..MinutesSettingsChange::default()
        },
    ] {
        let error = env
            .service
            .change_settings(change)
            .await
            .expect_err("refused");
        assert_eq!(error.code(), "MIN_SETTING_INVALID");
    }
    let status = env
        .service
        .change_settings(MinutesSettingsChange {
            delete_recordings_after_save: Some(true),
            ..MinutesSettingsChange::default()
        })
        .await
        .expect("change");
    assert!(status.settings.delete_recordings_after_save);
    let file: serde_json::Value = serde_json::from_slice(
        &std::fs::read(env.minutes_folder().join("settings.json")).expect("settings"),
    )
    .expect("json");
    assert_eq!(file["delete_recordings_after_save"], true);
}

// Error Table: MIN_TRANSCRIPT_TOO_LONG
#[tokio::test]
async fn error_table_min_transcript_too_long() {
    let env = Env::start(Setup::default()).await;
    let mut vtt = String::from("WEBVTT\n\n");
    let line = "word ".repeat(700);
    for n in 0..130u32 {
        vtt.push_str(&format!(
            "00:{:02}:{:02}.000 --> 00:{:02}:{:02}.500\n<v Priya>{}</v>\n\n",
            n / 60,
            n % 60,
            n / 60,
            n % 60,
            line.trim()
        ));
    }
    let long = env.dir.0.join("long.vtt");
    std::fs::write(&long, vtt).expect("write");
    let job = env.service.import(env.ws, long).await.expect("import");
    let job = env.until_finished(&job, 20).await;
    assert_eq!(failed_code(&job), "MIN_TRANSCRIPT_TOO_LONG");
    assert_eq!(env.calls(), 0);
}

// Error Table: MIN_STORE_FAILED
#[tokio::test]
async fn error_table_min_store_failed() {
    let env = Env::start(Setup::default()).await;
    // A file where the transcripts folder should be: the transcript cannot be written.
    std::fs::create_dir_all(env.workspace_folder()).expect("workspace");
    std::fs::write(env.workspace_folder().join("transcripts"), b"in the way").expect("block");
    let job = env
        .service
        .import(env.ws, fixture("meeting.vtt"))
        .await
        .expect("import");
    let job = env.until_finished(&job, 20).await;
    assert_eq!(failed_code(&job), "MIN_STORE_FAILED");
    assert_eq!(env.calls(), 0);
}

// Error Table: MIN_WRITE_FAILED is `error_writing_fails`; a transcription error keeps its code.
#[tokio::test]
async fn error_table_transcription_codes_pass_through() {
    let env = Env::start(Setup::default()).await;
    let error = env
        .service
        .import(env.ws, env.dir.0.join("slides.pdf"))
        .await
        .expect_err("format");
    assert_eq!(error.code(), "TRX_FORMAT_UNSUPPORTED");
    let empty = env.dir.0.join("empty.txt");
    std::fs::write(&empty, b"").expect("write");
    let job = env.service.import(env.ws, empty).await.expect("import");
    let job = env.until_finished(&job, 20).await;
    assert!(failed_code(&job).starts_with("TRX_"));
}

// Error Table: a recorder error keeps its code.
#[tokio::test]
async fn error_table_recorder_codes_pass_through() {
    let env = Env::start(Setup::default()).await;
    env.fake.obs().recording = true;
    let error = env.service.record(env.ws).await.expect_err("already");
    assert_eq!(error.code(), "OBS_ALREADY_RECORDING");
    env.fake.close();
    let error = env.service.record(env.ws).await.expect_err("closed");
    assert_eq!(error.code(), "OBS_NOT_REACHABLE");
    assert!(matches!(
        error,
        MinutesError::Recorder(ObsError::NotReachable)
    ));
}

// D6 and E2, E3: deleting a recording keeps the minutes; removing takes the files with them.
#[tokio::test]
async fn delete_recording_remove_and_workspace_removed() {
    let env = Env::start(Setup::default()).await;
    let recording = ordin_types::id::RecordingId::new();
    let recordings = env.workspace_folder().join("recordings");
    std::fs::create_dir_all(&recordings).expect("recordings");
    let file = recordings.join(format!("{recording}.mkv"));
    std::fs::write(&file, b"matroska").expect("recording");
    let minutes = env
        .journal
        .keep_minutes(NewMinutes {
            workspace_id: env.ws,
            title: "Stand-up".to_owned(),
            source: MinutesSource::Recording,
            source_name: format!("{recording}.mkv"),
            body: BODY.to_owned(),
            recording: Some(recording),
            run_id: None,
        })
        .await
        .expect("keep");
    let after = env
        .service
        .delete_recording(minutes.id)
        .await
        .expect("delete");
    assert_eq!(after.recording, None);
    assert!(!file.exists());

    let transcripts = env.workspace_folder().join("transcripts");
    std::fs::create_dir_all(&transcripts).expect("transcripts");
    let kept = transcripts.join(format!("{}.json", minutes.id));
    std::fs::write(&kept, b"{}").expect("transcript");
    env.service.remove(minutes.id).await.expect("remove");
    assert!(!kept.exists());
    assert!(env
        .journal
        .minutes(minutes.id)
        .await
        .expect("journal")
        .is_none());

    env.keep().await;
    env.service
        .workspace_removed(env.ws)
        .await
        .expect("workspace removed");
    assert!(!env.workspace_folder().exists());
    assert!(env
        .journal
        .list_minutes(env.ws, None)
        .await
        .expect("list")
        .items
        .is_empty());
}

// A1: an unparsable settings file is put aside and the defaults are used.
#[tokio::test]
async fn an_unreadable_settings_file_is_reset() {
    let env = Env::start(Setup::default()).await;
    let folder = env.minutes_folder();
    std::fs::write(folder.join("settings.json"), b"{ not json").expect("write");
    std::fs::write(folder.join("left.json.part"), b"crash").expect("part");
    let service = MinutesService::new(
        folder.clone(),
        TranscriptionService::new(
            env.models.clone(),
            localisation().await,
            reqwest::Client::new(),
        ),
        env.service_recorder().await,
        env.journal.clone(),
        localisation().await,
        env.writer.clone(),
    )
    .expect("service");
    assert!(folder.join("settings.json.bad").is_file());
    assert!(!folder.join("left.json.part").exists());
    let status = service.status(env.ws).await.expect("status");
    assert_eq!(status.settings.obs_port, 4455);
    assert!(!status.writer_notice_seen);
}

// C1: consent is kept; a failed download fails the waiting job, and consent stays given.
#[tokio::test]
async fn consent_is_kept_and_a_failed_download_fails_the_waiting_jobs() {
    let env = Env::start(Setup::default()).await;
    let job = env
        .service
        .import(env.ws, fixture("speech-60s.wav"))
        .await
        .expect("import");
    env.until(&job, 20, |s| *s == MinutesJobState::WaitingForModel)
        .await;
    // The models folder is a file: the download cannot even begin.
    std::fs::remove_dir_all(&env.models).expect("remove");
    std::fs::write(&env.models, b"in the way").expect("block");
    env.service.give_model_consent().await.expect("consent");
    let job = env.until_finished(&job, 60).await;
    assert!(failed_code(&job).starts_with("TRX_"));
    assert!(
        env.service
            .status(env.ws)
            .await
            .expect("status")
            .model_consent_given
    );
}

#[test]
fn the_transcript_renders_one_line_per_utterance() {
    use ordin_dictation::transcription::{Transcript, TranscriptSourceKind, Utterance};
    let transcript = Transcript {
        format: 1,
        source_kind: TranscriptSourceKind::Recording,
        source_name: "x.mkv".to_owned(),
        language: None,
        duration_ms: 3_725_000,
        utterances: vec![
            Utterance {
                start_ms: 0,
                end_ms: 900,
                speaker: Some("You".to_owned()),
                text: "Hello.".to_owned(),
            },
            Utterance {
                start_ms: 3_723_500,
                end_ms: 3_724_000,
                speaker: None,
                text: "Bye.".to_owned(),
            },
        ],
    };
    assert_eq!(
        render(&transcript),
        "[00:00:00] You: Hello.\n[01:02:03] Bye.\n"
    );
}

#[test]
fn every_error_has_its_code_retry_flag_and_repair() {
    let cases: Vec<(MinutesError, &str, bool)> = vec![
        (MinutesError::NotFound, "MIN_NOT_FOUND", false),
        (
            MinutesError::WorkspaceUnknown,
            "MIN_WORKSPACE_UNKNOWN",
            false,
        ),
        (MinutesError::JobNotFound, "MIN_JOB_NOT_FOUND", false),
        (MinutesError::JobFinished, "MIN_JOB_FINISHED", false),
        (
            MinutesError::RecordingNotFound,
            "MIN_RECORDING_NOT_FOUND",
            false,
        ),
        (
            MinutesError::RecordingInProgress,
            "MIN_RECORDING_IN_PROGRESS",
            false,
        ),
        (MinutesError::EditConflict, "MIN_EDIT_CONFLICT", false),
        (MinutesError::BodyInvalid, "MIN_BODY_INVALID", false),
        (MinutesError::SettingInvalid, "MIN_SETTING_INVALID", false),
        (
            MinutesError::TranscriptTooLong,
            "MIN_TRANSCRIPT_TOO_LONG",
            false,
        ),
        (MinutesError::StoreFailed, "MIN_STORE_FAILED", true),
        (MinutesError::WriteFailed, "MIN_WRITE_FAILED", true),
        (
            MinutesError::Recorder(ObsError::Timeout),
            "OBS_TIMEOUT",
            true,
        ),
        (MinutesError::JobNotReady, "MIN_JOB_NOT_READY", false),
        (
            MinutesError::Journal(ordin_journal::JournalError::Unavailable),
            "JRNL_DATASTORE_UNAVAILABLE",
            true,
        ),
    ];
    for (error, code, retryable) in cases {
        assert_eq!(error.code(), code);
        assert_eq!(error.retryable(), retryable, "{code}");
        if code.starts_with("MIN_") {
            assert_eq!(
                error.repair(),
                retryable.then_some(RepairAction::TryAgain),
                "{code}"
            );
        }
    }
    // 13 with MIN_JOB_NOT_READY (MinutesService.md MIN-TR-3).
    assert_eq!(crate::ALL_CODES.len(), 13);
}

#[test]
fn job_states_serialise_with_their_tag() {
    let done = MinutesJobState::Done {
        minutes_id: MinutesId::new(),
    };
    assert_eq!(serde_json::to_value(&done).expect("json")["state"], "done");
    assert_eq!(
        serde_json::to_value(MinutesJobState::WaitingForModel).expect("json"),
        serde_json::json!({ "state": "waiting_for_model" })
    );
    let progress = MinutesJobState::DownloadingModel {
        received_mb: 3,
        total_mb: 1533,
    };
    assert_eq!(
        serde_json::to_value(progress).expect("json"),
        serde_json::json!({ "state": "downloading_model", "received_mb": 3, "total_mb": 1533 })
    );
}

// --- Text for Ordin from any recording (MIN-TR-1 to MIN-TR-5) ------------------------------

// Scenario: Happy path — a recording becomes text and nothing else
// spec: ordin-desktop/components/MinutesService#MIN-TR-2
#[tokio::test]
#[ignore = "needs ggml-medium.bin in ORDIN_SPEECH_MODELS; run by plan 07's exit criteria"]
async fn happy_path_a_recording_becomes_text_and_nothing_else() {
    let env = Env::start(Setup {
        real_models: true,
        ..Setup::default()
    })
    .await;
    let job = env
        .service
        .transcribe(env.ws, fixture("speech-60s.m4a"))
        .await
        .expect("transcribe");
    assert_eq!(job.purpose, crate::MinutesJobPurpose::Transcript);
    let job = env.until_finished(&job, 900).await;
    assert_eq!(job.state, MinutesJobState::TranscriptReady);
    let transcript = env
        .service
        .transcription(job.id)
        .await
        .expect("transcription");
    assert!(!transcript.utterances.is_empty());
    assert_eq!(env.calls(), 0, "no minutes were written");
    let kept = env
        .journal
        .list_minutes(env.ws, None)
        .await
        .expect("journal");
    assert!(kept.items.is_empty(), "the Journal holds no new minutes");
    assert_eq!(
        env.service.cancel_job(job.id).await.err().map(|e| e.code()),
        Some("MIN_JOB_FINISHED")
    );
}

// Scenario: Edge — consent is still the person's
// spec: ordin-desktop/components/MinutesService#MIN-TR-2
#[tokio::test]
async fn edge_text_for_ordin_waits_for_the_persons_consent() {
    let env = Env::start(Setup::default()).await;
    let job = env
        .service
        .transcribe(env.ws, fixture("speech-60s.m4a"))
        .await
        .expect("transcribe");
    env.until(&job, 20, |s| *s == MinutesJobState::WaitingForModel)
        .await;
    assert!(files_in(&env.models).is_empty(), "nothing was downloaded");
    assert!(
        !env.service
            .status(env.ws)
            .await
            .expect("status")
            .model_consent_given
    );
}

// Scenario: Error path — reading too early
// spec: ordin-desktop/components/MinutesService#MIN-TR-3
#[tokio::test]
async fn error_table_min_job_not_ready() {
    let env = Env::start(Setup::default()).await;
    let waiting = env
        .service
        .transcribe(env.ws, fixture("speech-60s.m4a"))
        .await
        .expect("transcribe");
    env.until(&waiting, 20, |s| *s == MinutesJobState::WaitingForModel)
        .await;
    assert_eq!(
        env.service
            .transcription(waiting.id)
            .await
            .err()
            .map(|e| e.code()),
        Some("MIN_JOB_NOT_READY")
    );
    // A minutes job is never text for Ordin, even when done.
    let minutes = env
        .service
        .import(env.ws, fixture("meeting.vtt"))
        .await
        .expect("import");
    let minutes = env.until_finished(&minutes, 20).await;
    done_id(&minutes);
    assert_eq!(
        env.service
            .transcription(minutes.id)
            .await
            .err()
            .map(|e| e.code()),
        Some("MIN_JOB_NOT_READY")
    );
    assert_eq!(
        env.service
            .transcription(ordin_types::id::MinutesJobId::new())
            .await
            .err()
            .map(|e| e.code()),
        Some("MIN_JOB_NOT_FOUND")
    );
}

// Scenario: Error path — a file that is not a recording
// spec: ordin-desktop/components/MinutesService#MIN-TR-1
#[tokio::test]
async fn error_text_for_ordin_from_a_file_that_is_not_a_recording() {
    let env = Env::start(Setup::default()).await;
    let error = env
        .service
        .transcribe(env.ws, fixture("meeting.docx"))
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "TRX_FORMAT_UNSUPPORTED");
    assert!(env.service.jobs(env.ws).await.expect("jobs").is_empty());
}

// MinutesService.md MIN-MACH-1 to MIN-MACH-4: the speech model chosen by the machine. The machine's
// recommendation is set through the test seam, since the test server's own answer is fixed.

// Scenario: A new person gets the model the machine suits
// spec: ordin-desktop/components/MinutesService#MIN-MACH-1
// spec: ordin-desktop/components/MinutesService#MIN-MACH-3
#[tokio::test]
async fn scenario_a_new_person_gets_the_model_the_machine_suits() {
    let env = Env::start(Setup {
        settings_file: false,
        ..Setup::default()
    })
    .await;
    env.service.set_recommended(SpeechModelChoice::Smaller);
    let status = env.service.status(env.ws).await.expect("status");
    assert_eq!(
        status.settings.speech_model,
        SpeechModelSetting::Recommended
    );
    assert_eq!(status.recommended_model, SpeechModelChoice::Smaller);
    assert_eq!(status.model.choice, SpeechModelChoice::Smaller);
    let wire = serde_json::to_value(&status).expect("serialise");
    assert_eq!(wire["settings"]["speech_model"], "recommended");
    assert_eq!(wire["recommended_model"], "smaller");
    assert_eq!(wire["model"]["choice"], "smaller");
}

// Scenario: The person's choice wins
// spec: ordin-desktop/components/MinutesService#MIN-MACH-2
#[tokio::test]
async fn scenario_the_persons_choice_wins() {
    let env = Env::start(Setup::default()).await;
    env.service.set_recommended(SpeechModelChoice::Smaller);
    env.service
        .change_settings(MinutesSettingsChange {
            speech_model: Some(SpeechModelSetting::Smaller),
            ..MinutesSettingsChange::default()
        })
        .await
        .expect("smaller");
    let status = env
        .service
        .change_settings(MinutesSettingsChange {
            speech_model: Some(SpeechModelSetting::Larger),
            ..MinutesSettingsChange::default()
        })
        .await
        .expect("larger");
    assert_eq!(status.model.choice, SpeechModelChoice::Larger);
    // The next audio job asks for the larger model (1,533 MB), not the recommended one.
    let audio = env
        .service
        .import(env.ws, fixture("speech-60s.m4a"))
        .await
        .expect("import");
    env.until(&audio, 20, |s| *s == MinutesJobState::WaitingForModel)
        .await;
    let status = env.service.status(env.ws).await.expect("status");
    assert_eq!(status.model.choice, SpeechModelChoice::Larger);
    assert_eq!(status.model.size_mb, 1533);
    // The smaller model, present, is not used.
    std::fs::write(env.models.join("ggml-small.bin"), b"not used").expect("small");
    let audio = env
        .service
        .import(env.ws, fixture("speech-60s.m4a"))
        .await
        .expect("import");
    env.until(&audio, 20, |s| *s == MinutesJobState::WaitingForModel)
        .await;
}

// Scenario: Back to the recommendation
// spec: ordin-desktop/components/MinutesService#MIN-MACH-3
#[tokio::test]
async fn scenario_back_to_the_recommendation() {
    let env = Env::start(Setup {
        consent_given: true,
        ..Setup::default()
    })
    .await;
    env.service.set_recommended(SpeechModelChoice::Smaller);
    let status = env
        .service
        .change_settings(MinutesSettingsChange {
            speech_model: Some(SpeechModelSetting::Recommended),
            ..MinutesSettingsChange::default()
        })
        .await
        .expect("recommended");
    assert_eq!(
        status.settings.speech_model,
        SpeechModelSetting::Recommended
    );
    assert_eq!(status.model.choice, SpeechModelChoice::Smaller);
    assert_eq!(status.model.file_name, "ggml-small.bin");
    assert!(!status.model.present);
    // Consent was given, so the job does not wait for the person: the smaller model's download
    // begins at once. The models folder is a file, so the download fails without the network.
    std::fs::remove_dir_all(&env.models).expect("remove");
    std::fs::write(&env.models, b"in the way").expect("block");
    let audio = env
        .service
        .import(env.ws, fixture("speech-60s.wav"))
        .await
        .expect("import");
    let job = env.until_finished(&audio, 60).await;
    assert!(failed_code(&job).starts_with("TRX_"));
    std::fs::remove_file(&env.models).expect("unblock");
    // With the smaller model present, the next audio job goes past the model check to the
    // speech engine, never waiting for a model.
    std::fs::create_dir_all(&env.models).expect("models");
    std::fs::write(env.models.join("ggml-small.bin"), b"not a model").expect("small");
    let audio = env
        .service
        .import(env.ws, fixture("speech-60s.wav"))
        .await
        .expect("import");
    let job = env.until_finished(&audio, 60).await;
    assert!(failed_code(&job).starts_with("TRX_"));
    assert!(
        env.service
            .status(env.ws)
            .await
            .expect("status")
            .model
            .present
    );
}

// Scenario: An older settings file keeps its choice
// spec: ordin-desktop/components/MinutesService#MIN-MACH-1
#[tokio::test]
async fn scenario_an_older_settings_file_keeps_its_choice() {
    // `Env::start` writes the file as it was before MIN-MACH-1, with `"speech_model": "larger"`.
    let env = Env::start(Setup::default()).await;
    env.service.set_recommended(SpeechModelChoice::Smaller);
    let status = env.service.status(env.ws).await.expect("status");
    assert_eq!(status.settings.speech_model, SpeechModelSetting::Larger);
    assert_eq!(status.model.choice, SpeechModelChoice::Larger);
    assert!(!env.minutes_folder().join("settings.json.bad").exists());
}

// MIN-MACH-4: a value outside the three puts the file aside, as an unparsable one.
// spec: ordin-desktop/components/MinutesService#MIN-MACH-4
#[tokio::test]
async fn error_a_speech_model_outside_the_three_values_resets_the_file() {
    let env = Env::start(Setup::default()).await;
    let folder = env.minutes_folder();
    std::fs::write(
        folder.join("settings.json"),
        serde_json::json!({
            "format": 1,
            "speech_model": "tiny",
            "delete_recordings_after_save": false,
            "obs_port": 4455,
            "model_consent_at": null,
            "writer_notice_seen_at": null,
        })
        .to_string(),
    )
    .expect("write");
    let service = MinutesService::new(
        folder.clone(),
        TranscriptionService::new(
            env.models.clone(),
            localisation().await,
            reqwest::Client::new(),
        ),
        env.service_recorder().await,
        env.journal.clone(),
        localisation().await,
        env.writer.clone(),
    )
    .expect("service");
    assert!(folder.join("settings.json.bad").is_file());
    let status = service.status(env.ws).await.expect("status");
    assert_eq!(
        status.settings.speech_model,
        SpeechModelSetting::Recommended
    );
    let refused: Result<MinutesSettingsChange, _> =
        serde_json::from_value(serde_json::json!({ "speech_model": "tiny" }));
    assert!(refused.is_err());
}
