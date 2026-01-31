//! The Minutes Service (MinutesService.md *Internal Logic* A–E): one job loop, the two one-time
//! agreements, recording with OBS, and the minutes pages' files.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use ordin_dictation::transcription::{
    recommended_model, DownloadProgress, SpeakerTracks, SpeechModelChoice, SpeechModelInfo,
    TranscribeProgress, Transcript, TranscriptSourceKind, TranscriptionError, TranscriptionService,
};
use ordin_journal::{Journal, JournalError, MinutesUpdate};
use ordin_localisation::{Localisation, MessageArgs};
use ordin_obs::{ObsError, ObsRecorder, ObsState};
use ordin_types::id::{MinutesId, MinutesJobId, RecordingId, WorkspaceId};
use ordin_types::time::Timestamp;
use ordin_types::wire::minutes::{Minutes, MinutesSource};
use ordin_types::CodedError;
use tokio::sync::{watch, Notify};
use tokio_util::sync::CancellationToken;

use crate::error::MinutesError;
use crate::files::{
    self, RecordingFile, SettingsFile, WorkspaceFolders, LOWEST_PORT, RECORDING_FILE, SETTINGS_FILE,
};
use crate::types::{
    MinutesJob, MinutesJobPurpose, MinutesJobState, MinutesSettingsChange, MinutesStatus,
    MinutesWriter, RecordingView, WriteMinutesRequest,
};

/// B5: a rendered transcript longer than this is refused.
pub const MAX_TRANSCRIPT_CHARS: usize = 400_000;
/// E1: an edited body larger than this is refused.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// B7: Ordin's run must end within this time.
const WRITE_LIMIT: Duration = Duration::from_secs(15 * 60);
/// D5: a recording stopped on quit must be queued within this time.
const SHUTDOWN_STOP_LIMIT: Duration = Duration::from_secs(15);
/// A1: orphaned transcript files older than this are deleted.
const ORPHAN_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
/// B3, C1: progress reaches the job at most once a second.
const PROGRESS_EVERY: Duration = Duration::from_secs(1);

/// One job and what the loop needs to run it.
struct Job {
    view: MinutesJob,
    /// The file the job reads: the imported file, or the recording.
    path: PathBuf,
    recording: Option<RecordingId>,
    /// Recordings only: both tracks were recorded.
    others_heard: bool,
    cancel: CancellationToken,
    /// The transcript file is written; the job goes on at B5 (B6, `retry`).
    from_transcript: bool,
}

struct State {
    settings: SettingsFile,
    /// The model this computer handles well (TRX-MACH-3), read once in `new`.
    recommended: SpeechModelChoice,
    jobs: Vec<Job>,
    recording: Option<RecordingFile>,
    downloading: bool,
}

impl State {
    /// MIN-MACH-2: the recommendation for `recommended`, else the person's choice; resolved each
    /// time it is read, so a change of setting applies to the next job (C3).
    // spec: ordin-desktop/components/MinutesService#MIN-MACH-2
    fn model_in_use(&self) -> SpeechModelChoice {
        self.settings.speech_model.resolve(self.recommended)
    }
}

pub struct MinutesService {
    me: Weak<MinutesService>,
    folder: PathBuf,
    transcription: Arc<TranscriptionService>,
    recorder: Arc<ObsRecorder>,
    journal: Arc<Journal>,
    localisation: Arc<Localisation>,
    writer: Arc<dyn MinutesWriter>,
    state: Mutex<State>,
    /// A job was queued.
    wake: Notify,
    /// One of `record` and `stop` at a time.
    recording_op: tokio::sync::Mutex<()>,
}

impl MinutesService {
    /// A1: creates the minutes folder, reads the settings, sets the OBS port and deletes the
    /// `*.part` files a crash left.
    pub fn new(
        folder: PathBuf,
        transcription: Arc<TranscriptionService>,
        recorder: Arc<ObsRecorder>,
        journal: Arc<Journal>,
        localisation: Arc<Localisation>,
        writer: Arc<dyn MinutesWriter>,
    ) -> Result<Arc<Self>, MinutesError> {
        std::fs::create_dir_all(&folder).map_err(|_| MinutesError::StoreFailed)?;
        files::remove_parts(&folder);
        let settings = files::read_settings(&folder);
        recorder.set_port(settings.obs_port)?;
        Ok(Arc::new_cyclic(|me| MinutesService {
            me: me.clone(),
            folder,
            transcription,
            recorder,
            journal,
            localisation,
            writer,
            state: Mutex::new(State {
                settings,
                recommended: recommended_model(),
                jobs: Vec::new(),
                recording: None,
                downloading: false,
            }),
            wake: Notify::new(),
            recording_op: tokio::sync::Mutex::new(()),
        }))
    }

    /// A2: the job loop, oldest queued job first, one at a time, until `shutdown`. Before the
    /// first job it queues again the stopped recordings that have no minutes yet and deletes the
    /// orphaned transcript files of A1 (both need the Journal, which `new` does not wait for).
    pub async fn run(self: &Arc<Self>, shutdown: CancellationToken) {
        self.queue_unfinished_recordings().await;
        self.remove_orphaned_transcripts().await;
        loop {
            let woken = self.wake.notified();
            let next =
                self.lock().jobs.iter().find_map(|job| {
                    (job.view.state == MinutesJobState::Queued).then_some(job.view.id)
                });
            match next {
                Some(id) => {
                    tokio::select! {
                        () = self.run_job(id) => {}
                        () = shutdown.cancelled() => return,
                    }
                }
                None => {
                    tokio::select! {
                        () = woken => {}
                        () = shutdown.cancelled() => return,
                    }
                }
            }
        }
    }

    pub async fn status(&self, workspace_id: WorkspaceId) -> Result<MinutesStatus, MinutesError> {
        // SPEC-GAP-1: MinutesService.md checks the workspace against the Journal, which has no
        // workspace lookup (the workspaces are the Workspace Service's); `MIN_WORKSPACE_UNKNOWN`
        // is left to the desktop program, which checks before calling (plan 07 FU-011).
        let _ = workspace_id;
        self.status_now().await
    }

    async fn status_now(&self) -> Result<MinutesStatus, MinutesError> {
        let (settings, model, recommended, consent, notice, recording) = {
            let state = self.lock();
            (
                state.settings.settings(),
                state.model_in_use(),
                state.recommended,
                state.settings.model_consent_at.is_some(),
                state.settings.writer_notice_seen_at.is_some(),
                state.recording.as_ref().map(RecordingFile::view),
            )
        };
        Ok(MinutesStatus {
            model: self.model_info(model).await?,
            // spec: ordin-desktop/components/MinutesService#MIN-MACH-3
            recommended_model: recommended,
            settings,
            model_consent_given: consent,
            writer_notice_seen: notice,
            writer_name: self.writer.writer_name(),
            recorder: self.recorder.state().await,
            recording,
        })
    }

    /// C3: a different model is used by later jobs; nothing is downloaded here.
    pub async fn change_settings(
        &self,
        change: MinutesSettingsChange,
    ) -> Result<MinutesStatus, MinutesError> {
        if change == MinutesSettingsChange::default()
            || change.obs_port.is_some_and(|port| port < LOWEST_PORT)
        {
            return Err(MinutesError::SettingInvalid);
        }
        {
            let mut state = self.lock();
            let mut settings = state.settings.clone();
            if let Some(model) = change.speech_model {
                settings.speech_model = model;
            }
            if let Some(delete) = change.delete_recordings_after_save {
                settings.delete_recordings_after_save = delete;
            }
            if let Some(port) = change.obs_port {
                settings.obs_port = port;
            }
            files::write_json(&self.folder.join(SETTINGS_FILE), &settings)?;
            state.settings = settings;
        }
        if let Some(port) = change.obs_port {
            self.recorder
                .set_port(port)
                .map_err(|_| MinutesError::SettingInvalid)?;
        }
        self.status_now().await
    }

    /// C1: consent is kept, then the chosen model is downloaded on a task.
    pub async fn give_model_consent(&self) -> Result<(), MinutesError> {
        self.save_settings(|s| s.model_consent_at = Some(Timestamp::now()))?;
        self.start_download();
        Ok(())
    }

    /// C4: refused while a job is transcribing; consent stays given.
    pub async fn remove_model(&self) -> Result<(), MinutesError> {
        let choice = {
            let state = self.lock();
            if state
                .jobs
                .iter()
                .any(|j| matches!(j.view.state, MinutesJobState::Transcribing { .. }))
            {
                return Err(TranscriptionError::Busy.into());
            }
            state.model_in_use()
        };
        Ok(self.transcription.remove_model(choice).await?)
    }

    /// B6: kept once; every job waiting for it goes on.
    pub async fn accept_writer_notice(&self) -> Result<(), MinutesError> {
        self.save_settings(|s| s.writer_notice_seen_at = Some(Timestamp::now()))?;
        let waiting: Vec<MinutesJobId> = self
            .lock()
            .jobs
            .iter()
            .filter(|j| j.view.state == MinutesJobState::WaitingForNotice)
            .map(|j| j.view.id)
            .collect();
        for id in waiting {
            self.set_state(id, MinutesJobState::Queued);
        }
        self.wake.notify_one();
        Ok(())
    }

    /// B1: the file is read where it is; it is never moved or deleted.
    pub async fn import(
        &self,
        workspace_id: WorkspaceId,
        path: PathBuf,
    ) -> Result<MinutesJob, MinutesError> {
        // SPEC-GAP-1: see `status`.
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let source = match extension.as_str() {
            "vtt" | "docx" | "txt" => MinutesSource::TranscriptFile,
            "m4a" | "mp3" | "wav" => MinutesSource::AudioFile,
            _ => return Err(TranscriptionError::FormatUnsupported.into()),
        };
        let source_name = file_name(&path);
        Ok(self.queue(
            workspace_id,
            source,
            source_name,
            path,
            None,
            MinutesJobPurpose::Minutes,
        ))
    }

    /// MIN-TR-1: a recording becomes text for Ordin; no minutes are written.
    // spec: ordin-desktop/components/MinutesService#MIN-TR-1
    pub async fn transcribe(
        &self,
        workspace_id: WorkspaceId,
        path: PathBuf,
    ) -> Result<MinutesJob, MinutesError> {
        // SPEC-GAP-1: see `status`.
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !matches!(extension.as_str(), "m4a" | "mp3" | "wav" | "mkv") {
            return Err(TranscriptionError::FormatUnsupported.into());
        }
        let source_name = file_name(&path);
        Ok(self.queue(
            workspace_id,
            MinutesSource::AudioFile,
            source_name,
            path,
            None,
            MinutesJobPurpose::Transcript,
        ))
    }

    /// MIN-TR-3: the text of a finished `transcript` job.
    // spec: ordin-desktop/components/MinutesService#MIN-TR-3
    pub async fn transcription(&self, job_id: MinutesJobId) -> Result<Transcript, MinutesError> {
        let workspace_id = {
            let state = self.lock();
            let job = state
                .jobs
                .iter()
                .find(|j| j.view.id == job_id)
                .ok_or(MinutesError::JobNotFound)?;
            if job.view.purpose != MinutesJobPurpose::Transcript
                || job.view.state != MinutesJobState::TranscriptReady
            {
                return Err(MinutesError::JobNotReady);
            }
            job.view.workspace_id
        };
        read_transcript_file(
            &WorkspaceFolders::of(&self.folder, workspace_id).job_transcript(job_id),
        )
        .map_err(|_| MinutesError::StoreFailed)
    }

    pub async fn jobs(&self, workspace_id: WorkspaceId) -> Result<Vec<MinutesJob>, MinutesError> {
        Ok(self
            .lock()
            .jobs
            .iter()
            .filter(|j| j.view.workspace_id == workspace_id)
            .map(|j| j.view.clone())
            .collect())
    }

    /// B2, B6: a job not finished is cancelled; one waiting for the notice loses its transcript
    /// file.
    pub async fn cancel_job(&self, job_id: MinutesJobId) -> Result<MinutesJob, MinutesError> {
        let (view, transcript) = {
            let mut state = self.lock();
            let job = state
                .jobs
                .iter_mut()
                .find(|j| j.view.id == job_id)
                .ok_or(MinutesError::JobNotFound)?;
            if job.view.state.finished() {
                return Err(MinutesError::JobFinished);
            }
            // MIN-TR-4: a `transcript` job's file goes whenever one was written.
            let waited = job.view.state == MinutesJobState::WaitingForNotice
                || (job.view.purpose == MinutesJobPurpose::Transcript && job.from_transcript);
            job.cancel.cancel();
            job.view.state = MinutesJobState::Cancelled;
            let transcript = waited.then(|| {
                WorkspaceFolders::of(&self.folder, job.view.workspace_id).job_transcript(job_id)
            });
            (job.view.clone(), transcript)
        };
        log_job(&view);
        if let Some(transcript) = transcript {
            files::remove_if_present(&transcript)?;
        }
        Ok(view)
    }

    /// B7: a failed job is queued again from its kept transcript, else from its file.
    pub async fn retry(&self, job_id: MinutesJobId) -> Result<MinutesJob, MinutesError> {
        let view = {
            let mut state = self.lock();
            let job = state
                .jobs
                .iter_mut()
                .find(|j| j.view.id == job_id)
                .ok_or(MinutesError::JobNotFound)?;
            if !matches!(job.view.state, MinutesJobState::Failed { .. }) {
                return Err(MinutesError::JobFinished);
            }
            job.from_transcript = WorkspaceFolders::of(&self.folder, job.view.workspace_id)
                .job_transcript(job_id)
                .is_file();
            job.cancel = CancellationToken::new();
            job.view.state = MinutesJobState::Queued;
            job.view.clone()
        };
        log_job(&view);
        self.wake.notify_one();
        Ok(view)
    }

    /// D1: OBS must be ready; the folder it records into is kept for D4.
    pub async fn record(&self, workspace_id: WorkspaceId) -> Result<RecordingView, MinutesError> {
        // SPEC-GAP-1: see `status`.
        let _one = self.recording_op.lock().await;
        if self.lock().recording.is_some() {
            return Err(MinutesError::RecordingInProgress);
        }
        match self.recorder.state().await {
            ObsState::Ready { .. } => {}
            ObsState::NotInstalled => return Err(ObsError::NotInstalled.into()),
            ObsState::NotReachable => return Err(ObsError::NotReachable.into()),
            ObsState::PasswordNeeded => return Err(ObsError::PasswordNeeded.into()),
            ObsState::PasswordRefused => return Err(ObsError::PasswordRefused.into()),
            ObsState::VersionUnsupported { .. } => return Err(ObsError::VersionUnsupported.into()),
            ObsState::Recording { .. } => return Err(ObsError::AlreadyRecording.into()),
        }
        let started = self.recorder.start().await?;
        let recording = RecordingFile {
            format: 1,
            id: RecordingId::new(),
            workspace_id,
            started_at: started.started_at,
            others_heard: started.others_heard,
            record_folder: started.record_folder,
        };
        if let Err(error) = files::write_json(&self.folder.join(RECORDING_FILE), &recording) {
            // No trace could be kept: the recording is not left running without one.
            let _ = self.recorder.stop().await;
            return Err(error);
        }
        let view = recording.view();
        self.lock().recording = Some(recording);
        tracing::info!(
            component = "minutes",
            others_heard = view.others_heard,
            "minutes.recording_started"
        );
        Ok(view)
    }

    /// D2–D4: the file is moved into the workspace's recordings and a job is queued.
    pub async fn stop(&self, recording_id: RecordingId) -> Result<MinutesJob, MinutesError> {
        let _one = self.recording_op.lock().await;
        let recording = self
            .lock()
            .recording
            .clone()
            .filter(|r| r.id == recording_id)
            .ok_or(MinutesError::RecordingNotFound)?;
        let (output, duration_ms) = match self.recorder.stop().await {
            Ok(finished) => (finished.output_path, finished.duration_ms),
            // D4: OBS was closed during the recording.
            Err(ObsError::NotReachable) => {
                match files::newest_recording(&recording.record_folder, recording.started_at) {
                    Some(path) => (path, elapsed_ms(recording.started_at)),
                    None => {
                        self.forget_recording();
                        return Err(ObsError::OutputMissing.into());
                    }
                }
            }
            // OBS no longer records, or its file is gone: nothing is left to stop.
            Err(error @ (ObsError::NotRecording | ObsError::OutputMissing)) => {
                self.forget_recording();
                return Err(error.into());
            }
            Err(error) => return Err(error.into()),
        };
        let target =
            WorkspaceFolders::of(&self.folder, recording.workspace_id).recording(recording.id);
        // Only a recording is moved: a path OBS names that is not an `.mkv` is read where it is
        // and never deleted, so an answer from something posing as OBS cannot move another file.
        let is_recording = output
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mkv"));
        let moved = if is_recording {
            files::move_file(&output, &target)
        } else {
            Err(std::io::Error::other("not a recording"))
        };
        let path = match moved {
            Ok(()) => target,
            Err(_) => {
                // D3: the job reads the file where OBS wrote it.
                tracing::warn!(component = "minutes", "minutes.move_failed");
                output
            }
        };
        self.forget_recording();
        tracing::info!(
            component = "minutes",
            others_heard = recording.others_heard,
            duration_ms,
            "minutes.recording_stopped"
        );
        let source_name = file_name(&path);
        Ok(self.queue(
            recording.workspace_id,
            MinutesSource::Recording,
            source_name,
            path,
            Some((recording.id, recording.others_heard)),
            MinutesJobPurpose::Minutes,
        ))
    }

    pub async fn open_obs(&self) -> Result<(), MinutesError> {
        Ok(self.recorder.open_obs().await?)
    }

    /// The transcript kept with the minutes.
    pub async fn transcript(&self, minutes_id: MinutesId) -> Result<Transcript, MinutesError> {
        let minutes = self.minutes(minutes_id).await?;
        let path =
            WorkspaceFolders::of(&self.folder, minutes.workspace_id).minutes_transcript(minutes_id);
        // SPEC-GAP-2: the Error Table has no row for a kept transcript that is missing or
        // unreadable; missing answers `MIN_NOT_FOUND`, unreadable `TRX_TRANSCRIPT_MALFORMED`
        // (plan 07 FU-012).
        let bytes = std::fs::read(&path).map_err(|_| MinutesError::NotFound)?;
        serde_json::from_slice(&bytes).map_err(|_| TranscriptionError::TranscriptMalformed.into())
    }

    /// E1: the person may change the four headings.
    pub async fn save_edit(
        &self,
        minutes_id: MinutesId,
        body: String,
        base_revision: u32,
    ) -> Result<Minutes, MinutesError> {
        if body.is_empty() || body.len() > MAX_BODY_BYTES {
            return Err(MinutesError::BodyInvalid);
        }
        match self
            .journal
            .update_minutes_body(minutes_id, body, base_revision)
            .await
        {
            Ok(MinutesUpdate::Updated(minutes)) => Ok(minutes),
            Ok(MinutesUpdate::Stale { .. }) => Err(MinutesError::EditConflict),
            Ok(MinutesUpdate::NotFound) => Err(MinutesError::NotFound),
            Err(JournalError::Invalid { .. }) => Err(MinutesError::BodyInvalid),
            Err(error) => Err(journal_error(error)),
        }
    }

    /// D6: the minutes and their transcript stay.
    pub async fn delete_recording(&self, minutes_id: MinutesId) -> Result<Minutes, MinutesError> {
        let minutes = self.minutes(minutes_id).await?;
        let recording = minutes.recording.ok_or(MinutesError::RecordingNotFound)?;
        files::remove_if_present(
            &WorkspaceFolders::of(&self.folder, minutes.workspace_id).recording(recording),
        )?;
        self.journal
            .set_minutes_recording(minutes_id, None)
            .await
            .map_err(journal_error)
    }

    /// E2: the transcript and the recording go with the minutes.
    pub async fn remove(&self, minutes_id: MinutesId) -> Result<(), MinutesError> {
        let minutes = self.minutes(minutes_id).await?;
        let folders = WorkspaceFolders::of(&self.folder, minutes.workspace_id);
        files::remove_if_present(&folders.minutes_transcript(minutes_id))?;
        if let Some(recording) = minutes.recording {
            files::remove_if_present(&folders.recording(recording))?;
        }
        self.journal
            .remove_minutes(minutes_id)
            .await
            .map_err(journal_error)
    }

    /// E3.
    pub async fn workspace_removed(&self, workspace_id: WorkspaceId) -> Result<(), MinutesError> {
        let root = WorkspaceFolders::of(&self.folder, workspace_id).root;
        match std::fs::remove_dir_all(&root) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(MinutesError::StoreFailed),
        }
        self.journal
            .remove_workspace_minutes(workspace_id)
            .await
            .map(drop)
            .map_err(journal_error)
    }

    /// D5: a running recording is stopped and queued within 15 s.
    pub async fn shutdown(&self) {
        let running = self.lock().recording.as_ref().map(|r| r.id);
        if let Some(id) = running {
            let _ = tokio::time::timeout(SHUTDOWN_STOP_LIMIT, self.stop(id)).await;
        }
    }

    // --- B: one job -------------------------------------------------------------------------

    async fn run_job(&self, id: MinutesJobId) {
        let Some((view, path, recording, others_heard, cancel, from_transcript)) =
            self.with_job(id, |j| {
                (
                    j.view.clone(),
                    j.path.clone(),
                    j.recording,
                    j.others_heard,
                    j.cancel.clone(),
                    j.from_transcript,
                )
            })
        else {
            return;
        };
        let folders = WorkspaceFolders::of(&self.folder, view.workspace_id);
        let transcript_file = folders.job_transcript(id);
        let transcript = if from_transcript {
            match read_transcript_file(&transcript_file) {
                Ok(transcript) => transcript,
                Err(error) => return self.fail(id, &error),
            }
        } else {
            let transcript = match view.source {
                MinutesSource::TranscriptFile => {
                    if !self.set_state(id, MinutesJobState::Reading) {
                        return;
                    }
                    self.transcription
                        .read_transcript(&path)
                        .await
                        .map_err(MinutesError::from)
                }
                MinutesSource::AudioFile | MinutesSource::Recording => {
                    // B2.
                    let choice = self.lock().model_in_use();
                    match self.model_info(choice).await {
                        Ok(model) if model.present => {}
                        Ok(_) => return self.wait_for_model(id),
                        Err(error) => return self.fail(id, &error),
                    }
                    let tracks = if view.source == MinutesSource::Recording && others_heard {
                        SpeakerTracks::YouAndOthers
                    } else {
                        SpeakerTracks::Mixed
                    };
                    let kind = if view.source == MinutesSource::Recording {
                        TranscriptSourceKind::Recording
                    } else {
                        TranscriptSourceKind::AudioFile
                    };
                    self.transcribe_job(id, &path, kind, choice, tracks, cancel.clone())
                        .await
                }
            };
            let transcript = match transcript {
                Ok(transcript) => transcript,
                Err(error) => return self.fail(id, &error),
            };
            // B4.
            if let Err(error) = files::write_json(&transcript_file, &transcript) {
                return self.fail(id, &error);
            }
            self.with_job(id, |j| j.from_transcript = true);
            transcript
        };
        // MIN-TR-2: text for Ordin stops after the notice (B6), before B5 and B7.
        // spec: ordin-desktop/components/MinutesService#MIN-TR-2
        if view.purpose == MinutesJobPurpose::Transcript {
            if self.lock().settings.writer_notice_seen_at.is_none() {
                self.set_state(id, MinutesJobState::WaitingForNotice);
                return;
            }
            if self.set_state(id, MinutesJobState::TranscriptReady) {
                tracing::info!(
                    component = "minutes",
                    duration_ms = transcript.duration_ms,
                    utterances = transcript.utterances.len(),
                    "minutes.transcript_ready"
                );
            }
            return;
        }
        // B5.
        let text = render(&transcript);
        if text.chars().count() > MAX_TRANSCRIPT_CHARS {
            return self.fail(id, &MinutesError::TranscriptTooLong);
        }
        // B6.
        if self.lock().settings.writer_notice_seen_at.is_none() {
            self.set_state(id, MinutesJobState::WaitingForNotice);
            return;
        }
        // B7.
        if !self.set_state(id, MinutesJobState::Writing) {
            return;
        }
        let request = WriteMinutesRequest {
            workspace_id: view.workspace_id,
            job_id: id,
            source: view.source,
            source_name: view.source_name.clone(),
            recording,
            transcript_text: text,
        };
        let written =
            tokio::time::timeout(WRITE_LIMIT, self.writer.write(request, cancel.clone())).await;
        if cancel.is_cancelled() {
            return;
        }
        let minutes_id = match written {
            Ok(Ok(minutes_id)) => minutes_id,
            _ => return self.fail(id, &MinutesError::WriteFailed),
        };
        if std::fs::rename(&transcript_file, folders.minutes_transcript(minutes_id)).is_err() {
            tracing::warn!(
                component = "minutes",
                code = "MIN_STORE_FAILED",
                "minutes.failed"
            );
        }
        self.set_state(id, MinutesJobState::Done { minutes_id });
        tracing::info!(
            component = "minutes",
            source = source_name(view.source),
            duration_ms = transcript.duration_ms,
            utterances = transcript.utterances.len(),
            "minutes.kept"
        );
        // B8.
        let delete = self.lock().settings.delete_recordings_after_save;
        if let (MinutesSource::Recording, true, Some(recording)) = (view.source, delete, recording)
        {
            self.delete_kept_recording(minutes_id, &folders.recording(recording))
                .await;
        }
    }

    /// B3: progress is copied into the job at most once a second.
    async fn transcribe_job(
        &self,
        id: MinutesJobId,
        path: &Path,
        kind: TranscriptSourceKind,
        choice: SpeechModelChoice,
        tracks: SpeakerTracks,
        cancel: CancellationToken,
    ) -> Result<Transcript, MinutesError> {
        if !self.set_state(
            id,
            MinutesJobState::Transcribing {
                done_ms: 0,
                total_ms: 0,
            },
        ) {
            return Err(TranscriptionError::Cancelled.into());
        }
        let (sender, mut receiver) = watch::channel(TranscribeProgress::Decoding {
            done_ms: 0,
            total_ms: 0,
        });
        let copy = async {
            while receiver.changed().await.is_ok() {
                let (done_ms, total_ms) = match *receiver.borrow_and_update() {
                    TranscribeProgress::Decoding { done_ms, total_ms }
                    | TranscribeProgress::Recognising { done_ms, total_ms } => (done_ms, total_ms),
                };
                self.set_state(id, MinutesJobState::Transcribing { done_ms, total_ms });
                tokio::time::sleep(PROGRESS_EVERY).await;
            }
        };
        let work = self
            .transcription
            .transcribe_audio(path, kind, choice, tracks, sender, cancel);
        Ok(alongside(work, copy).await?)
    }

    /// B2: the job waits; with consent given the download starts at once.
    fn wait_for_model(&self, id: MinutesJobId) {
        if !self.set_state(id, MinutesJobState::WaitingForModel) {
            return;
        }
        if self.lock().settings.model_consent_at.is_some() {
            self.start_download();
        }
    }

    /// C1: one download at a time, its progress copied into every job waiting for the model.
    fn start_download(&self) {
        let choice = {
            let mut state = self.lock();
            if state.downloading {
                return;
            }
            state.downloading = true;
            state.model_in_use()
        };
        let Some(me) = self.me.upgrade() else {
            return;
        };
        tokio::spawn(async move {
            let (sender, mut receiver) = watch::channel(DownloadProgress {
                received_mb: 0,
                total_mb: 0,
            });
            let copy = async {
                while receiver.changed().await.is_ok() {
                    let progress = *receiver.borrow_and_update();
                    me.each_waiting_for_model(|job| {
                        job.view.state = MinutesJobState::DownloadingModel {
                            received_mb: progress.received_mb,
                            total_mb: progress.total_mb,
                        };
                    });
                    tokio::time::sleep(PROGRESS_EVERY).await;
                }
            };
            let work = me
                .transcription
                .download_model(choice, sender, CancellationToken::new());
            let result = alongside(work, copy).await;
            me.lock().downloading = false;
            let waiting: Vec<MinutesJobId> = {
                let state = me.lock();
                state
                    .jobs
                    .iter()
                    .filter(|j| waiting_for_model(&j.view.state))
                    .map(|j| j.view.id)
                    .collect()
            };
            for id in waiting {
                match &result {
                    Ok(_) => {
                        me.set_state(id, MinutesJobState::Queued);
                    }
                    Err(error) => me.fail(id, error),
                }
            }
            me.wake.notify_one();
        });
    }

    fn each_waiting_for_model(&self, mut change: impl FnMut(&mut Job)) {
        let mut state = self.lock();
        for job in state.jobs.iter_mut() {
            if waiting_for_model(&job.view.state) {
                change(job);
            }
        }
    }

    /// B8: the recording file goes; the minutes keep their transcript.
    async fn delete_kept_recording(&self, minutes_id: MinutesId, path: &Path) {
        if files::remove_if_present(path).is_err() {
            tracing::warn!(
                component = "minutes",
                code = "MIN_STORE_FAILED",
                "minutes.failed"
            );
            return;
        }
        match self.journal.set_minutes_recording(minutes_id, None).await {
            Ok(_) => tracing::info!(component = "minutes", "minutes.recording_deleted"),
            Err(error) => {
                tracing::warn!(component = "minutes", code = error.code(), "minutes.failed")
            }
        }
    }

    // --- A1, A2 ------------------------------------------------------------------------------

    /// A2: every recording in a workspace's `recordings/` that no minutes name is queued again.
    async fn queue_unfinished_recordings(&self) {
        for (workspace_id, root) in self.workspace_folders() {
            let folders = WorkspaceFolders { root };
            let Ok(entries) = std::fs::read_dir(folders.recordings()) else {
                continue;
            };
            let named = match self.recordings_named(workspace_id).await {
                Ok(named) => named,
                Err(error) => {
                    tracing::warn!(component = "minutes", code = error.code(), "minutes.failed");
                    continue;
                }
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(id) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<uuid::Uuid>().ok())
                    .map(RecordingId)
                else {
                    continue;
                };
                if path.extension().is_some_and(|e| e == "mkv") && !named.contains(&id) {
                    let name = file_name(&path);
                    self.queue(
                        workspace_id,
                        MinutesSource::Recording,
                        name,
                        path,
                        Some((id, true)),
                        MinutesJobPurpose::Minutes,
                    );
                }
            }
        }
    }

    /// The recordings the workspace's minutes name.
    async fn recordings_named(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<RecordingId>, JournalError> {
        let mut named = Vec::new();
        let mut cursor = None;
        loop {
            let page = self.journal.list_minutes(workspace_id, cursor).await?;
            for summary in &page.items {
                if let Some(recording) = self
                    .journal
                    .minutes(summary.id)
                    .await?
                    .and_then(|m| m.recording)
                {
                    named.push(recording);
                }
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return Ok(named),
            }
        }
    }

    /// A1: a transcript file whose name is no minutes in the Journal and that is older than 7
    /// days is deleted (a failed job's transcript is kept that long for **Try again**).
    async fn remove_orphaned_transcripts(&self) {
        for (_, root) in self.workspace_folders() {
            let folders = WorkspaceFolders { root };
            let Ok(entries) = std::fs::read_dir(folders.transcripts()) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let old = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age > ORPHAN_AGE);
                let Some(id) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<uuid::Uuid>().ok())
                else {
                    continue;
                };
                if old && matches!(self.journal.minutes(MinutesId(id)).await, Ok(None)) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    fn workspace_folders(&self) -> Vec<(WorkspaceId, PathBuf)> {
        let Ok(entries) = std::fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| {
                let id = e.file_name().to_str()?.parse::<uuid::Uuid>().ok()?;
                Some((WorkspaceId(id), e.path()))
            })
            .collect()
    }

    // --- helpers -------------------------------------------------------------------------------

    /// Tests only: the machine's recommendation, which `new` reads from this computer.
    #[cfg(test)]
    pub(crate) fn set_recommended(&self, choice: SpeechModelChoice) {
        self.lock().recommended = choice;
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_job<T>(&self, id: MinutesJobId, f: impl FnOnce(&mut Job) -> T) -> Option<T> {
        self.lock().jobs.iter_mut().find(|j| j.view.id == id).map(f)
    }

    /// Sets the state unless the job was cancelled; false when it was (or is gone).
    fn set_state(&self, id: MinutesJobId, state: MinutesJobState) -> bool {
        let view = self.with_job(id, |job| {
            if job.view.state == MinutesJobState::Cancelled {
                return None;
            }
            job.view.state = state;
            Some(job.view.clone())
        });
        match view.flatten() {
            Some(view) => {
                if !matches!(
                    view.state,
                    MinutesJobState::Transcribing { .. } | MinutesJobState::DownloadingModel { .. }
                ) {
                    log_job(&view);
                }
                true
            }
            None => false,
        }
    }

    /// `Failed` with the code and its sentence; logged `minutes.failed`.
    fn fail(&self, id: MinutesJobId, error: &dyn CodedError) {
        let code = error.code();
        let message = self.localisation.error_sentence(code, &MessageArgs::new());
        if self.set_state(
            id,
            MinutesJobState::Failed {
                code: code.to_owned(),
                message,
            },
        ) {
            tracing::warn!(component = "minutes", code, "minutes.failed");
        }
    }

    fn queue(
        &self,
        workspace_id: WorkspaceId,
        source: MinutesSource,
        source_name: String,
        path: PathBuf,
        // A recording and whether both of its tracks were recorded.
        recording: Option<(RecordingId, bool)>,
        purpose: MinutesJobPurpose,
    ) -> MinutesJob {
        let (recording, others_heard) = match recording {
            Some((id, heard)) => (Some(id), heard),
            None => (None, false),
        };
        let view = MinutesJob {
            id: MinutesJobId::new(),
            workspace_id,
            source_name,
            source,
            state: MinutesJobState::Queued,
            created_at: Timestamp::now(),
            purpose,
        };
        self.lock().jobs.push(Job {
            view: view.clone(),
            path,
            recording,
            others_heard,
            cancel: CancellationToken::new(),
            from_transcript: false,
        });
        log_job(&view);
        self.wake.notify_one();
        view
    }

    fn forget_recording(&self) {
        self.lock().recording = None;
        let _ = files::remove_if_present(&self.folder.join(RECORDING_FILE));
    }

    fn save_settings(&self, change: impl FnOnce(&mut SettingsFile)) -> Result<(), MinutesError> {
        let mut state = self.lock();
        let mut settings = state.settings.clone();
        change(&mut settings);
        files::write_json(&self.folder.join(SETTINGS_FILE), &settings)?;
        state.settings = settings;
        Ok(())
    }

    async fn model_info(&self, choice: SpeechModelChoice) -> Result<SpeechModelInfo, MinutesError> {
        self.transcription
            .models()
            .await
            .into_iter()
            .find(|m| m.choice == choice)
            .ok_or_else(|| TranscriptionError::ModelMissing.into())
    }

    async fn minutes(&self, id: MinutesId) -> Result<Minutes, MinutesError> {
        self.journal
            .minutes(id)
            .await
            .map_err(journal_error)?
            .ok_or(MinutesError::NotFound)
    }
}

/// B5: one line per utterance, `[hh:mm:ss] Speaker: text`, no `Speaker:` when none.
pub(crate) fn render(transcript: &Transcript) -> String {
    let mut text = String::new();
    for u in &transcript.utterances {
        let seconds = u.start_ms / 1000;
        text.push_str(&format!(
            "[{:02}:{:02}:{:02}] ",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        ));
        if let Some(speaker) = &u.speaker {
            text.push_str(speaker);
            text.push_str(": ");
        }
        text.push_str(&u.text);
        text.push('\n');
    }
    text
}

fn read_transcript_file(path: &Path) -> Result<Transcript, MinutesError> {
    let bytes = std::fs::read(path).map_err(|_| MinutesError::StoreFailed)?;
    serde_json::from_slice(&bytes).map_err(|_| MinutesError::StoreFailed)
}

fn waiting_for_model(state: &MinutesJobState) -> bool {
    matches!(
        state,
        MinutesJobState::WaitingForModel | MinutesJobState::DownloadingModel { .. }
    )
}

/// Runs `work` with `copy` beside it; `copy` may end first (its sender dropped), `work` decides
/// the result, and `copy` stops when `work` ends.
async fn alongside<T>(
    work: impl std::future::Future<Output = T>,
    copy: impl std::future::Future<Output = ()>,
) -> T {
    tokio::pin!(work);
    tokio::pin!(copy);
    let mut copying = true;
    loop {
        tokio::select! {
            result = &mut work => return result,
            () = &mut copy, if copying => copying = false,
        }
    }
}

fn journal_error(error: JournalError) -> MinutesError {
    match error {
        JournalError::NotFound => MinutesError::NotFound,
        error => MinutesError::Journal(error),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn elapsed_ms(since: Timestamp) -> u64 {
    u64::try_from(Timestamp::now().unix_millis() - since.unix_millis()).unwrap_or(0)
}

fn source_name(source: MinutesSource) -> &'static str {
    match source {
        MinutesSource::TranscriptFile => "transcript_file",
        MinutesSource::AudioFile => "audio_file",
        MinutesSource::Recording => "recording",
    }
}

/// `minutes.job`: id, source and state only; never a name, a path or text.
fn log_job(view: &MinutesJob) {
    tracing::info!(
        component = "minutes",
        job_id = %view.id,
        source = source_name(view.source),
        state = view.state.name(),
        "minutes.job"
    );
}
