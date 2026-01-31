# SPEC: Minutes Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** added (amended January 2026; user decision; Approved by the user January 2026; amended November 2026: the recording folder comes from `RecordingStarted.record_folder`, user decision) — meeting minutes from transcripts, recordings and OBS
> **Mode:** greenfield
> **derivedFromHld:** 0.72.0
> **Crate:** `ordin-minutes` · **Build phase:** 4 · **Programs:** D · **Build switch:** `minutes`
> **Dependencies:** Transcription Service (`ordin-dictation`, feature `transcription`), OBS Recorder Adapter, Journal and Artefact Store, Localisation. Reaches the Ordin Host only through the `MinutesWriter` trait the desktop program implements (no crate edge).

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Gaps](#gaps)

## Purpose

The Minutes Service turns a meeting into minutes. A person gives it a transcript file or a recording, or records the meeting with OBS from Ordin; the service gets a transcript from the Transcription Service, has Ordin write minutes with four sections — **Summary**, **Decisions**, **Actions** (each with an owner or "unassigned") and **Open questions** — and keeps them in the Journal as a minutes page of the workspace, listed under **Minutes** in the workspace tree. It owns the meeting jobs and their progress, the person's two one-time agreements (downloading the speech model; sending transcript text to the AI service that writes the minutes), the minutes settings, and the recordings and transcripts stored in Ordin's data folder.

## Approach

A job queue in one Tokio task: each job moves through fixed states and runs one at a time, so a long transcription never competes with another for the processor. Minutes are a Journal row kind, like dashboards, so they belong to a workspace, survive restarts, and list with a cursor; recordings and transcripts are files beside the Journal under the workspace's minutes folder. The writing is done by Ordin's own agent through the `MinutesWriter` trait, which the desktop program implements over `OrdinHost::write_minutes` (OrdinHost.md OH-MIN-1), so this crate does not depend on the Ordin Host. Rejected: *minutes as files in the Files store* — no workspace tree group or export row covers them, and edits would bypass the Journal; *minutes as knowledge pages* — the knowledge store is for the workspace's reference material and is shared with knowledge agents, whereas minutes may hold sensitive meeting content; *running jobs in parallel* — two hour-long transcriptions at once exceed the memory target.

## Shared Context

### Facts this component implements

- Inputs: transcript files `.vtt`, `.docx`, `.txt`; recordings `.m4a`, `.mp3`, `.wav`; Ordin's own OBS recordings (`.mkv`).
- Before the first audio transcription, Ordin states the speech model's size and folder and asks; declining downloads nothing and leaves transcript files working.
- Before the first minutes are written, a one-time notice names the AI service the transcript text is sent to; audio never leaves the computer (user decision, January 2026).
- Each press of **Record** shows a reminder to tell participants; an indicator stays visible while recording; **Stop** turns the recording into minutes with no further input.
- Recordings are kept in Ordin's data folder per workspace until the person deletes them; the setting **Delete recordings after the minutes are saved** (off by default) deletes them once the minutes are kept. The transcript is always kept with the minutes. A file the person imported is never moved or deleted.

### Paths (from the program)

| Name | Value |
|---|---|
| minutes folder | `<DesktopPaths::data_root>/minutes/` |
| settings file | `<minutes folder>/settings.json` |
| workspace folder | `<minutes folder>/<workspace_id>/` with `recordings/` and `transcripts/` |

### Settings (`settings.json`, `format` 1, written atomically)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `speech_model` | `"larger"` \| `"smaller"` | `"larger"` | model used for audio |
| `delete_recordings_after_save` | bool | `false` | delete a recording once its minutes are kept |
| `obs_port` | integer 1024–65535 | `4455` | OBS WebSocket port |
| `model_consent_at` | timestamp \| null | `null` | when the person agreed to the download |
| `writer_notice_seen_at` | timestamp \| null | `null` | when the person accepted the notice |

### Shared types (2C wire §23, copied)

```rust
pub struct MinutesId(pub uuid::Uuid);           // UUIDv7
pub struct MinutesJobId(pub uuid::Uuid);        // UUIDv7
pub struct RecordingId(pub uuid::Uuid);         // UUIDv7

#[serde(rename_all = "snake_case")]
pub enum MinutesSource { TranscriptFile, AudioFile, Recording }

pub struct Minutes {
    pub id: MinutesId, pub workspace_id: WorkspaceId,
    pub title: String,                      // 1–120 chars
    pub source: MinutesSource, pub source_name: String,   // file name, 1–255 chars
    pub body: String,                       // Markdown, ≤ 1 MiB, the four headings
    pub recording: Option<RecordingId>,     // None once deleted, or for imports
    pub run_id: Option<RunId>, pub revision: u32,         // revision ≥ 1
    pub kept_at: Timestamp, pub edited_at: Option<Timestamp>,
}
pub struct MinutesSummary { pub id: MinutesId, pub title: String, pub source: MinutesSource, pub kept_at: Timestamp }

#[serde(tag = "state", rename_all = "snake_case")]
pub enum MinutesJobState {
    Queued, WaitingForModel, DownloadingModel { received_mb: u32, total_mb: u32 },
    Reading, Transcribing { done_ms: u64, total_ms: u64 }, WaitingForNotice,
    Writing, Done { minutes_id: MinutesId }, Failed { code: String, message: String }, Cancelled,
}
pub struct MinutesJob { pub id: MinutesJobId, pub workspace_id: WorkspaceId, pub source_name: String, pub source: MinutesSource, pub state: MinutesJobState, pub created_at: Timestamp }

pub struct MinutesStatus {
    pub settings: MinutesSettings,          // the table above, without the two timestamps
    pub model: SpeechModelInfo,             // the chosen model
    pub model_consent_given: bool, pub writer_notice_seen: bool,
    pub writer_name: String,                // the AI service named in the notice
    pub recorder: ObsState,                 // ObsRecorderAdapter.md
    pub recording: Option<RecordingView>,
}
pub struct RecordingView { pub id: RecordingId, pub workspace_id: WorkspaceId, pub started_at: Timestamp, pub others_heard: bool }
```

### Interfaces consumed

- Transcription Service: `models`, `download_model`, `remove_model`, `read_transcript`, `transcribe_audio` (TranscriptionService.md).
- OBS Recorder Adapter: `state`, `open_obs`, `start`, `stop`, `set_port` (ObsRecorderAdapter.md).
- Journal (JournalAndArtefactStore.md JA-MIN-1): `minutes`, `list_minutes`, `update_minutes_body`, `set_minutes_recording`, `remove_minutes`, `remove_workspace_minutes`. Minutes are written by the Ordin Host's `keep_minutes` tool, not here.
- `MinutesWriter` (below), implemented by the desktop program over `OrdinHost::write_minutes`.

## Public Interface

```rust
#[async_trait::async_trait]
pub trait MinutesWriter: Send + Sync {
    /// The name shown in the one-time notice: the AI service Ordin's runs use.
    fn writer_name(&self) -> String;
    /// Runs Ordin's minutes request and waits for its end; Ok with the id `keep_minutes` returned.
    async fn write(&self, req: WriteMinutesRequest, cancel: CancellationToken) -> Result<MinutesId, MinutesError>;
}

pub struct WriteMinutesRequest {
    pub workspace_id: WorkspaceId, pub job_id: MinutesJobId,
    pub source: MinutesSource, pub source_name: String,
    pub recording: Option<RecordingId>,
    pub transcript_text: String,          // rendered by §B5, ≤ 400,000 chars
}

pub struct MinutesService { /* private */ }

impl MinutesService {
    pub fn new(folder: PathBuf, transcription: Arc<TranscriptionService>, recorder: Arc<ObsRecorder>,
               journal: Arc<Journal>, localisation: Arc<Localisation>, writer: Arc<dyn MinutesWriter>)
               -> Result<Arc<Self>, MinutesError>;
    pub async fn run(self: &Arc<Self>, shutdown: CancellationToken);       // the job loop

    pub async fn status(&self, workspace_id: WorkspaceId) -> Result<MinutesStatus, MinutesError>;
    pub async fn change_settings(&self, change: MinutesSettingsChange) -> Result<MinutesStatus, MinutesError>;
    pub async fn give_model_consent(&self) -> Result<(), MinutesError>;    // then downloads, see §C
    pub async fn remove_model(&self) -> Result<(), MinutesError>;
    pub async fn accept_writer_notice(&self) -> Result<(), MinutesError>;

    pub async fn import(&self, workspace_id: WorkspaceId, path: PathBuf) -> Result<MinutesJob, MinutesError>;
    pub async fn jobs(&self, workspace_id: WorkspaceId) -> Result<Vec<MinutesJob>, MinutesError>;
    pub async fn cancel_job(&self, job_id: MinutesJobId) -> Result<MinutesJob, MinutesError>;
    /// A `Failed` job is queued again from its kept transcript (or its file); any other state → `JobFinished` or `JobNotFound`.
    pub async fn retry(&self, job_id: MinutesJobId) -> Result<MinutesJob, MinutesError>;

    pub async fn record(&self, workspace_id: WorkspaceId) -> Result<RecordingView, MinutesError>;
    pub async fn stop(&self, recording_id: RecordingId) -> Result<MinutesJob, MinutesError>;
    pub async fn open_obs(&self) -> Result<(), MinutesError>;

    pub async fn transcript(&self, minutes_id: MinutesId) -> Result<Transcript, MinutesError>;
    pub async fn save_edit(&self, minutes_id: MinutesId, body: String, base_revision: u32) -> Result<Minutes, MinutesError>;
    pub async fn delete_recording(&self, minutes_id: MinutesId) -> Result<Minutes, MinutesError>;
    pub async fn remove(&self, minutes_id: MinutesId) -> Result<(), MinutesError>;
    pub async fn workspace_removed(&self, workspace_id: WorkspaceId) -> Result<(), MinutesError>;
    /// Called by the program on quit: a running recording is stopped and queued (§D5).
    pub async fn shutdown(&self);
}

pub struct MinutesSettingsChange { pub speech_model: Option<SpeechModelChoice>, pub delete_recordings_after_save: Option<bool>, pub obs_port: Option<u16> }

#[derive(Debug, thiserror::Error)]
pub enum MinutesError {
    #[error("minutes not found")]           NotFound,             // MIN_NOT_FOUND
    #[error("workspace unknown")]           WorkspaceUnknown,     // MIN_WORKSPACE_UNKNOWN
    #[error("job not found")]               JobNotFound,          // MIN_JOB_NOT_FOUND
    #[error("job already finished")]        JobFinished,          // MIN_JOB_FINISHED
    #[error("recording not found")]         RecordingNotFound,    // MIN_RECORDING_NOT_FOUND
    #[error("already recording")]           RecordingInProgress,  // MIN_RECORDING_IN_PROGRESS
    #[error("edited elsewhere")]            EditConflict,         // MIN_EDIT_CONFLICT
    #[error("body invalid")]                BodyInvalid,          // MIN_BODY_INVALID
    #[error("setting invalid")]             SettingInvalid,       // MIN_SETTING_INVALID
    #[error("transcript too long")]         TranscriptTooLong,    // MIN_TRANSCRIPT_TOO_LONG
    #[error("storage failed")]              StoreFailed,          // MIN_STORE_FAILED, retryable
    #[error("minutes not written")]         WriteFailed,          // MIN_WRITE_FAILED, retryable
    #[error(transparent)] Transcription(#[from] TranscriptionError), // its TRX_ code
    #[error(transparent)] Recorder(#[from] ObsError),                // its OBS_ code
    #[error("journal: {0}")] Journal(JournalError),                  // its JRNL_ code
}
```

### Who calls each public item

| Item | Called by |
|---|---|
| `new`, `run`, `shutdown`, `workspace_removed` | desktop program, with `minutes` |
| every other method | desktop program, for the `minutes.*` commands (ServiceApi.md SVC-MIN-1), called by the screens (ScreensModule.md SM-MIN-1 to SM-MIN-10) |

### Example

```json
// minutes.import reply (an .m4a, model not yet downloaded)
{"id":"0192b3c4-3333-7000-8000-000000000010","workspace_id":"0192b3c4-0000-7000-8000-000000000001",
 "source_name":"Supplier call.m4a","source":"audio_file","state":{"state":"waiting_for_model"},
 "created_at":"2026-01-14T10:02:11Z"}
```

Minutes body kept for it:

```markdown
## Summary
The team reviewed the supplier renewal and agreed to stay with Northwind for 2026.

## Decisions
- Renew the Northwind contract for twelve months.

## Actions
- Send the renewal letter — Priya Shah
- Check the delivery penalties clause — unassigned

## Open questions
- Should the renewal include the new warehouse?
```

## Internal Logic

### A. Start

1. `new`: create the minutes folder; read `settings.json` (absent → defaults; unparsable → renamed to `settings.json.bad`, defaults used, `minutes.settings_reset` warn). Call `recorder.set_port(obs_port)`. Delete `*.part` and orphaned transcript files whose job is not in the Journal and is older than 7 days.
2. `run`: the job loop takes `Queued` jobs oldest first, one at a time. Jobs live in memory; a job not `Done` when Ordin quits is lost, except a stopped recording (§D5), which is queued again at the next start from its file in `recordings/`.

### B. A job

1. `import(ws, path)`: workspace unknown to the Journal → `WorkspaceUnknown`. Extension: `vtt`/`docx`/`txt` → source `TranscriptFile`; `m4a`/`mp3`/`wav` → `AudioFile`; else `Transcription(FormatUnsupported)`. Create the job `Queued` and return it.
2. **Model check** (audio and recordings): chosen model not present → `WaitingForModel`. When `model_consent_at` is set, the download starts at once (§C); otherwise the job waits for the person's answer. Cancelling a waiting job leaves transcript-file jobs unaffected.
3. **Transcript**: `TranscriptFile` → `Reading`, `read_transcript`; audio → `Transcribing`, `transcribe_audio(path, kind, speech_model, Mixed or YouAndOthers for recordings, …)`, with progress copied into the job state at most once per second. The job's `CancellationToken` is passed. Errors → `Failed` with the code and its sentence.
4. Write the `Transcript` as JSON to `transcripts/<job_id>.json` (atomic). Failure → `Failed` `MIN_STORE_FAILED`.
5. **Render** for the writer: one line per utterance `[hh:mm:ss] Speaker: text` (no `Speaker:` when none). Over 400,000 characters → `Failed` `MIN_TRANSCRIPT_TOO_LONG`.
6. **Notice**: `writer_notice_seen_at` null → `WaitingForNotice` until `accept_writer_notice`; cancelling deletes the transcript file.
7. **Writing**: `writer.write(req, cancel)` with a limit of 15 minutes. `Ok(id)` → rename `transcripts/<job_id>.json` to `transcripts/<minutes_id>.json`; `Done { minutes_id }`; log `minutes.kept` (info: `source`, `duration_ms`, `utterances`). Error or limit → `Failed` `MIN_WRITE_FAILED` (retryable: the screen's **Try again** sends `minutes.retry`, which queues the same job again from step B6 when its transcript file exists, else from step B2 with its source file). The transcript file stays until the job is retried or 7 days pass.
8. **Retention**: source `Recording` and `delete_recordings_after_save` true → delete the recording file, then `set_minutes_recording(id, None)`; log `minutes.recording_deleted` (info).

### C. Speech model

1. `give_model_consent`: set `model_consent_at` to now, then start `download_model(speech_model)` on a task, copying its progress into every `WaitingForModel` job as `DownloadingModel`. Success → those jobs go on at step B3. Failure → those jobs `Failed` with the code; consent stays given and **Try again** downloads again.
2. Declining is no call at all: the screens send `minutes.cancel_job`; nothing is downloaded.
3. `change_settings` with a different `speech_model` sets it for later jobs; it does not download until the next audio job (with consent already given it downloads then, without asking again).
4. `remove_model`: no job transcribing; delete the chosen model's file; consent stays given.

### D. Recording

1. `record(ws)`: a recording already running → `RecordingInProgress`. `recorder.state()` not `Ready` → the matching `Recorder` error. `recorder.start()`; keep its `record_folder` in memory and in `recording.json` (for D4). Keep `RecordingView { id: new, workspace_id, started_at, others_heard }` in memory and in `<minutes folder>/recording.json` (so a crash leaves a trace). Log `minutes.recording_started` (info).
2. `stop(id)`: unknown id → `RecordingNotFound`. `recorder.stop()` → `output_path`. Move the file into `<ws>/recordings/<recording_id>.mkv` (rename; across volumes copy, `fsync`, then delete the original). Delete `recording.json`. Queue a job with source `Recording`, `source_name` = the moved file's name and `recording = Some(id)`; return it. No further input is asked of the person; the notice of B6 applies only if never accepted.
3. Move failure → the file is left where OBS wrote it and the job reads it there; logged `minutes.move_failed` (warn).
4. `stop` answering `NotReachable` (OBS closed meanwhile): use the newest `.mkv` in the `record_folder` kept at D1 modified after `started_at`; none → `Recorder(OutputMissing)`.
5. Quit while recording: `shutdown` runs D2 with a limit of 15 s; the queued job resumes at the next start (A2).
6. `delete_recording(minutes_id)`: delete the file; `set_minutes_recording(id, None)`; the minutes and transcript stay.

### E. Minutes pages

1. `save_edit`: body empty or over 1 MiB → `BodyInvalid`; `update_minutes_body(id, body, base_revision)` → `Stale` → `EditConflict`; `NotFound` → `NotFound`. The four headings may be changed by the person; they are required only of the agent (OH-MIN-1).
2. `remove`: delete the transcript file and the recording file (when present), then `remove_minutes(id)`.
3. `workspace_removed`: delete `<ws>/` and call `remove_workspace_minutes(ws)`.

## Data Model

No table of its own: the `minutes` table is the Journal's (JournalAndArtefactStore.md JA-MIN-1). Files: `settings.json`, `recording.json` (one running recording at most), `<ws>/recordings/<recording_id>.mkv`, `<ws>/transcripts/<minutes_id>.json` (format 1, the `Transcript` of TranscriptionService.md). Rollback: deleting the `minutes` folder loses recordings and transcripts but no other data; no destructive schema change.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Minutes not in the Journal | `Err(permanent)` | `MIN_NOT_FOUND` | `{"code":"MIN_NOT_FOUND","message":"Those minutes are no longer here.","action":null}` |
| Workspace unknown | `Err(permanent)` | `MIN_WORKSPACE_UNKNOWN` | `{"code":"MIN_WORKSPACE_UNKNOWN","message":"That workspace is no longer here.","action":null}` |
| Job id unknown | `Err(permanent)` | `MIN_JOB_NOT_FOUND` | `{"code":"MIN_JOB_NOT_FOUND","message":"That meeting is no longer being worked on.","action":null}` |
| Cancel of a job already done, failed or cancelled, or retry of a job that has not failed | `Err(permanent)` | `MIN_JOB_FINISHED` | `{"code":"MIN_JOB_FINISHED","message":"That meeting has already finished.","action":null}` |
| Recording id unknown | `Err(permanent)` | `MIN_RECORDING_NOT_FOUND` | `{"code":"MIN_RECORDING_NOT_FOUND","message":"That recording is no longer here.","action":null}` |
| Record while recording | `Err(permanent)` | `MIN_RECORDING_IN_PROGRESS` | `{"code":"MIN_RECORDING_IN_PROGRESS","message":"Ordin is already recording. Stop that recording first.","action":null}` |
| Saved over a newer version | `Err(permanent)` | `MIN_EDIT_CONFLICT` | `{"code":"MIN_EDIT_CONFLICT","message":"These minutes changed since you opened them. Reopen them to see the latest.","action":null}` |
| Body empty or over 1 MiB | `Err(permanent)` | `MIN_BODY_INVALID` | `{"code":"MIN_BODY_INVALID","message":"The minutes are empty or too long to save.","action":null}` |
| Setting outside its values | `Err(permanent)` | `MIN_SETTING_INVALID` | `{"code":"MIN_SETTING_INVALID","message":"That setting can't be used.","action":null}` |
| Transcript over 400,000 characters | `Err(permanent)` | `MIN_TRANSCRIPT_TOO_LONG` | `{"code":"MIN_TRANSCRIPT_TOO_LONG","message":"That meeting is too long for one set of minutes.","action":null}` |
| A file in the minutes folder could not be written | `Err(retryable)` | `MIN_STORE_FAILED` | `{"code":"MIN_STORE_FAILED","message":"Ordin couldn't save the meeting on this computer. Free some space and try again.","action":"try_again"}` |
| Ordin's run failed, ended without keeping minutes, or passed 15 minutes | `Err(retryable)` | `MIN_WRITE_FAILED` | `{"code":"MIN_WRITE_FAILED","message":"Ordin couldn't write the minutes. Try again.","action":"try_again"}` |
| Transcription or recorder error | as its owner | `TRX_*`, `OBS_*` | as TranscriptionService.md, ObsRecorderAdapter.md |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Minutes Service

  Scenario: Happy path — minutes from a WebVTT transcript
    Given the writer notice was accepted and a scripted MinutesWriter that keeps minutes
    When import is called with the fixture "meeting.vtt"
    Then the job goes Reading, Writing, Done
    And the Journal holds minutes whose body has the headings Summary, Decisions, Actions and Open questions
    And each Actions item ends with an owner or "unassigned"

  Scenario: Edge — consent before the first audio
    Given no model and no consent
    When import is called with "speech-60s.m4a"
    Then the job is waiting_for_model and nothing is downloaded
    When the person cancels that job and imports "meeting.vtt"
    Then minutes are kept for the transcript and the models folder is still empty

  Scenario: Edge — the one-time notice
    Given the writer notice was never accepted
    When a transcript job reaches the writing stage
    Then the job is waiting_for_notice and the writer is not called
    When accept_writer_notice is called
    Then the writer is called once, and later jobs never wait for the notice

  Scenario: Happy path — record, stop, minutes with no further input
    Given a fake OBS that returns "/tmp/obs/2026-01-14 10-02-11.mkv" on StopRecord
    When record and then stop are called
    Then the file is moved to the workspace's recordings folder
    And a job with source recording runs to Done without any other call

  Scenario: Edge — delete recordings after the minutes are saved
    Given delete_recordings_after_save is true
    When a recording's minutes are kept
    Then the recording file is gone, the minutes' recording is none and the transcript is kept

  Scenario: Error — writing fails
    Given a MinutesWriter that returns an error
    When a transcript job runs
    Then the job is failed with MIN_WRITE_FAILED and its transcript file remains

  Scenario: Error — save over a newer version
    Given minutes at revision 3
    When save_edit is called with base_revision 2
    Then the error is MIN_EDIT_CONFLICT

  Scenario: Edge — an imported file is never moved or deleted
    Given an imported "Supplier call.m4a" and delete_recordings_after_save true
    When its minutes are kept
    Then "Supplier call.m4a" is still where it was
```

## Performance, Security, Observability

- **Performance targets:** every command except `import`'s work and `stop` answers within 200 ms p95; `stop` within 4 s p95 (OBS plus the move on one volume); job state changes reach `minutes.jobs` within 1 s. Peak memory is the Transcription Service's (one job at a time).
- **Security:** commands are desktop-only (`Local`); paths come only from the program's own dialog or OBS's answer, never from the screens; files are written under the minutes folder only, atomically; transcript text leaves the computer only through `MinutesWriter` after the notice was accepted; no transcript text, speaker name, minutes body or file path is logged; the OBS password is never seen by this crate.
- **Observability** (`component = "minutes"`): `minutes.job` (info: `job_id`, `source`, `state`), `minutes.kept` (info: `source`, `duration_ms`, `utterances`), `minutes.recording_started` / `minutes.recording_stopped` (info: `others_heard`, `duration_ms`), `minutes.recording_deleted` (info), `minutes.move_failed` (warn), `minutes.settings_reset` (warn), `minutes.failed` (warn: `code`).

## Gaps

None.

## Amendment 2026-11-21 — Text for Ordin from any recording (MIN-TR-1 to MIN-TR-5)

> **Status:** Approved by the user, 2026-11-21 ("approve as written"). Every section above stays in force. Caller: the desktop program's `MinutesPort` for Ordin's tools (OrdinHost.md OH-MIN-5, OrdinDesktop.md DSK-MIN-6). Why a job: Ordin's tool calls are stopped after 300 s (McpFront.md `call_timeout`), and a 60-minute recording takes up to 45 minutes to transcribe (SA-MIN-03), so the work runs in the job loop the person already sees, and Ordin reads the result later.

- **MIN-TR-1 `transcribe`.**

```rust
impl MinutesService {
    /// A job of purpose `Transcript`: the recording becomes text and no minutes are written.
    pub async fn transcribe(&self, workspace_id: WorkspaceId, path: PathBuf) -> Result<MinutesJob, MinutesError>;
    /// The text of a finished `Transcript` job (MIN-TR-3).
    pub async fn transcription(&self, job_id: MinutesJobId) -> Result<Transcript, MinutesError>;
}
```

  Workspace unknown → `WorkspaceUnknown`. Extension `m4a`, `mp3`, `wav` → source `AudioFile`; `mkv` → source `AudioFile` too (a Matroska file Ordin did not record); any other → `Transcription(FormatUnsupported)`. Create the job `Queued` with `purpose: Transcript` and return it. `import` (§B1) keeps `purpose: Minutes`.
- **MIN-TR-2 The job's steps.** A `Transcript` job runs §B2 (model check, with the person's consent exactly as for minutes: nothing is downloaded without it), §B3 with `SpeakerTracks::Mixed`, §B4 (`transcripts/<job_id>.json`) and §B6 (the one-time notice, because Ordin sends the text to the AI service when it reads it), then ends in the new state `TranscriptReady` instead of §B5 and §B7. No minutes are kept and no `MinutesWriter` call is made; §B8 does not apply. Log `minutes.transcript_ready` (info: `duration_ms`, `utterances`).
- **MIN-TR-3 `transcription`.** Job unknown → `JobNotFound`. Job of purpose `Minutes`, or not `TranscriptReady` → `JobNotReady`. Otherwise read `transcripts/<job_id>.json`; missing or unreadable → `StoreFailed`. The file stays for the 7 days of §A1, after which a job still listed answers `StoreFailed`; a job lost at quit (§A2) answers `JobNotFound`.
- **MIN-TR-4 Cancel and retry.** `cancel_job` on a `TranscriptReady` job → `JobFinished` (it is finished). `retry` of a failed `Transcript` job queues it again from §B2 with its source file. Cancelling a `Transcript` job deletes its transcript file when one was written.
- **MIN-TR-5 Shared types.** `MinutesJob` gains `purpose: MinutesJobPurpose` (`#[serde(default)]`, `minutes`); `MinutesJobState` gains `TranscriptReady` (2C wire WIRE-MIN-7). `MinutesError` gains `JobNotReady`.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `transcription` of a minutes job, or of a job not yet ready | `Err(permanent)` | `MIN_JOB_NOT_READY` | `{"code":"MIN_JOB_NOT_READY","message":"That recording is not text yet.","action":null}` |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Text for Ordin from any recording (MIN-TR-1 to MIN-TR-5)

  Scenario: Happy path — a recording becomes text and nothing else
    Given the chosen model is present and the notice was accepted
    When transcribe is called with "standup.m4a"
    Then the job passes reading the recording and ends transcript_ready
    And transcription returns its utterances
    And the Journal holds no new minutes

  Scenario: Edge — consent is still the person's
    Given the chosen model is absent and no consent was given
    When transcribe is called with "standup.m4a"
    Then the job waits for the model and nothing is downloaded

  Scenario: Error path — reading too early
    Given a transcript job that is transcribing
    When transcription is called for it
    Then the error is MIN_JOB_NOT_READY

  Scenario: Error path — a file that is not a recording
    When transcribe is called with "notes.docx"
    Then the error is TRX_FORMAT_UNSUPPORTED and no job starts
```

## Amendment 2026-11-22 — The speech model chosen by the machine (MIN-MACH-1 to MIN-MACH-4)

> **Status:** Approved by the user, 2026-11-22 ("approve as written"). Every section above stays in force except where this section replaces it. Source: the user's decision of 2026-11-22 (plan 07 FU-022); TranscriptionService.md TRX-MACH-1 to TRX-MACH-4.

- **MIN-MACH-1 The setting.** In *Settings (`settings.json`)*, `speech_model` becomes `"recommended"` \| `"larger"` \| `"smaller"`, default `"recommended"` (2C wire WIRE-MIN-8). `"recommended"` means the model `TranscriptionService::recommended_model()` names; `"larger"` and `"smaller"` are the person's own choice and win over the recommendation. A file written before this amendment keeps its `"larger"` or `"smaller"`, read as the person's choice. Any other value → `settings.json.bad` as for an unparsable file (§A1).
- **MIN-MACH-2 The model in use.** Wherever this spec reads `speech_model` to pick a model — B3 (`transcribe_audio`), C1 (`download_model`), C4 (`remove_model`) and `MinutesStatus.model` — it uses the *model in use*: the recommendation for `"recommended"`, else the chosen value. It is resolved when it is read, so a change of setting applies to the next job as C3 says.
- **MIN-MACH-3 Status.** `MinutesStatus` gains `recommended_model: SpeechModelChoice`; `model` stays the info of the model in use. `change_settings` accepts `"recommended"`; returning to `"recommended"` when the recommendation names a model not yet present behaves as C3 (it downloads at the next audio job, without asking again once consent is given).
- **MIN-MACH-4 Errors.** None new. `MIN_SETTING_INVALID` covers a `speech_model` outside the three values.

```gherkin
Feature: The speech model chosen by the machine (MIN-MACH-1 to MIN-MACH-4)

  Scenario: A new person gets the model the machine suits
    Given no settings.json and a machine for which recommended_model is Smaller
    When minutes.status is read
    Then settings.speech_model is "recommended", recommended_model is "smaller" and model.choice is "smaller"

  Scenario: The person's choice wins
    Given recommended_model is Smaller
    When change_settings sets speech_model "larger"
    Then the next audio job transcribes with the larger model

  Scenario: Back to the recommendation
    Given speech_model is "larger" and consent was given
    When change_settings sets speech_model "recommended" and recommended_model is Smaller
    Then the next audio job downloads the smaller model without asking and transcribes with it

  Scenario: An older settings file keeps its choice
    Given a settings.json written before this amendment with speech_model "larger"
    When the service starts
    Then settings.speech_model is "larger"
```
