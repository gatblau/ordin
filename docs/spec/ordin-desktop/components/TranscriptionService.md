# SPEC: Transcription Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** added (amended January 2026; user decision; Approved by the user January 2026) — meeting minutes from transcripts, recordings and OBS
> **Mode:** greenfield
> **derivedFromHld:** 0.72.0
> **Crate:** `ordin-dictation`, module `transcription`, behind the crate feature `transcription` · **Build phase:** 3 · **Programs:** D · **Build switch:** `minutes`
> **Dependencies:** Localisation

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

The Transcription Service turns one meeting file into a `Transcript`: a list of timed utterances with an optional speaker. It reads transcript files written by meeting tools (`.vtt`, `.docx`, `.txt`) directly, and turns recordings (`.m4a`, `.mp3`, `.wav`, and the `.mkv` files of Ordin's own recordings) into text on the computer with a downloaded speech model of the whisper.cpp class. It downloads that model only after the person has agreed, checks it against a SHA-256 checksum, and never sends audio or text anywhere. It is used only by the Minutes Service. It is separate from the Dictation Service in the same crate: dictation stays on the operating system's own recogniser (macOS 26 and later) and keeps no model; this service is the only part of Ordin that keeps a speech model.

## Approach

A plain Rust module compiled only with the crate feature `transcription`. Transcript files are parsed by hand-written line grammars (WebVTT cues, `.docx` paragraphs read with the workspace's `zip` and `quick-xml`, plain text). Audio is decoded with `symphonia` (formats MP3, AAC in MP4/M4A, WAV/PCM, Matroska), mixed or split per track, resampled to 16 kHz mono `f32` by an in-crate windowed-sinc resampler, and recognised by whisper.cpp through the `whisper-rs` binding, with Metal on Apple silicon (conditional on C-108: if the licence gate refuses `whisper-rs`, the module declares the six whisper.cpp C functions it calls itself and `build.rs` compiles whisper.cpp, MIT-licensed, from a pinned source archive with the same flags). Rejected: *the operating system's recogniser* — Windows has none Ordin may use (ADR-078) and macOS's is for live dictation, not hour-long files; *a cloud recogniser* — breaks the local-only rule; *`rubato` for resampling* — not in `Cargo.lock`, and one fixed 16 kHz conversion needs about 120 lines.

## Shared Context

### Facts this component implements

- Audio and transcripts never leave the computer. The only network request this component makes is the speech-model download, to the address in its bundled model list, after the person agreed (Minutes Service consent).
- Default model: the larger model (about 1.5 GB, more accurate); a setting switches to the smaller model (about 470 MB). Both are whisper.cpp-class models (user decision, January 2026).
- Speaker labels: Ordin's own recordings have two tracks, labelled **You** (track 1, the microphone) and **Others** (track 2, the computer's sound); imported transcripts keep the speaker names written in the file; imported audio has no speaker labels.
- macOS and Windows behave the same.

### Paths (from the program)

| Name | Value | Owner |
|---|---|---|
| models folder | `<DesktopPaths::data_root>/speech-models/` | this component (created on first download) |

### Constants *(chosen here)*

| Name | Value |
|---|---|
| `MAX_TRANSCRIPT_FILE_BYTES` | 20 MiB |
| `MAX_AUDIO_FILE_BYTES` | 4 GiB |
| `MAX_AUDIO_DURATION_MS` | 14,400,000 (4 hours) |
| `PROGRESS_INTERVAL_MS` | 1,000 |
| `CANCEL_DEADLINE_MS` | 5,000 |
| `DOWNLOAD_TIMEOUT_S` | 30 per read; no overall limit |
| `DOWNLOAD_RETRIES` | 3, backoff 2 s, 4 s, 8 s with ±20 % jitter |
| `SAMPLE_RATE_HZ` | 16,000 |

### Localisation interface consumed

```rust
impl Localisation { pub fn message(&self, id: MessageId, args: &MessageArgs) -> String; }
```

### Error contract

Module errors implement `ordin_types::CodedError` (`code`, `retryable`, `repair`); `RepairAction` ∈ {`TryAgain`, `SignInAgain`} (2C).

### Names chosen here

| Name | Kind |
|---|---|
| `ordin_dictation::transcription` | module, crate feature `transcription = ["dep:symphonia", "dep:whisper-rs", "dep:zip", "dep:quick-xml", "dep:sha2", "dep:reqwest"]` |
| `crates/ordin-dictation/speech-models.json` | bundled model list (`include_str!`) |
| `TranscriptionService`, `Transcript`, `Utterance`, `TranscriptSourceKind`, `SpeechModelChoice`, `SpeechModelInfo`, `SpeakerTracks`, `TranscribeProgress`, `DownloadProgress`, `TranscriptionError` | public types |
| `TRX_*` | error codes (Error Table) |
| `errors-transcription.ftl` | en-GB catalogue file of the `TRX_` sentences |

## Public Interface

```rust
use std::{path::{Path, PathBuf}, sync::Arc};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechModelChoice { Larger, Smaller }   // default Larger

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpeechModelInfo {
    pub choice: SpeechModelChoice,
    pub file_name: String,      // from speech-models.json, e.g. "ggml-medium.bin"
    pub size_mb: u32,           // shown in the consent prompt
    pub folder: PathBuf,        // the models folder, shown in the consent prompt
    pub present: bool,          // file exists and its checksum was verified when it was written
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSourceKind { TranscriptFile, AudioFile, Recording }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Utterance {
    pub start_ms: u64,
    pub end_ms: u64,                 // ≥ start_ms
    pub speaker: Option<String>,     // 1–80 chars; None for imported audio
    pub text: String,                // 1–4,000 chars, trimmed, single spaces
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    pub format: u32,                 // 1
    pub source_kind: TranscriptSourceKind,
    pub source_name: String,         // file name only, 1–255 chars
    pub language: Option<String>,    // BCP 47 tag detected by the model; None for transcript files
    pub duration_ms: u64,
    pub utterances: Vec<Utterance>,  // ordered by start_ms, then speaker; at least one
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerTracks {
    /// Every track mixed to one; no speaker label (imported audio).
    Mixed,
    /// Ordin's own recordings: track 1 → "You", track 2 → "Others".
    YouAndOthers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum TranscribeProgress {
    Decoding { done_ms: u64, total_ms: u64 },
    Recognising { done_ms: u64, total_ms: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadProgress { pub received_mb: u32, pub total_mb: u32 }

pub struct TranscriptionService { /* private */ }

impl TranscriptionService {
    pub fn new(models_folder: PathBuf, localisation: Arc<Localisation>, http: reqwest::Client) -> Arc<Self>;

    /// Both models, from speech-models.json; never touches the network.
    pub async fn models(&self) -> Vec<SpeechModelInfo>;

    /// Called only after the person agreed. Downloads to `<file>.part`, verifies SHA-256, renames.
    pub async fn download_model(&self, choice: SpeechModelChoice,
        progress: watch::Sender<DownloadProgress>, cancel: CancellationToken)
        -> Result<SpeechModelInfo, TranscriptionError>;

    /// Deletes the model file; Ok when absent.
    pub async fn remove_model(&self, choice: SpeechModelChoice) -> Result<(), TranscriptionError>;

    /// `.vtt`, `.docx`, `.txt` only.
    pub async fn read_transcript(&self, path: &Path) -> Result<Transcript, TranscriptionError>;

    /// `.m4a`, `.mp3`, `.wav`, `.mkv` only. Needs the chosen model present.
    pub async fn transcribe_audio(&self, path: &Path, source_kind: TranscriptSourceKind,
        model: SpeechModelChoice, tracks: SpeakerTracks,
        progress: watch::Sender<TranscribeProgress>, cancel: CancellationToken)
        -> Result<Transcript, TranscriptionError>;
}

#[derive(Debug, thiserror::Error)]
pub enum TranscriptionError {
    #[error("format not supported")]   FormatUnsupported,      // TRX_FORMAT_UNSUPPORTED
    #[error("file unreadable")]        FileUnreadable,         // TRX_FILE_UNREADABLE
    #[error("file too large")]         FileTooLarge,           // TRX_FILE_TOO_LARGE
    #[error("transcript malformed")]   TranscriptMalformed,    // TRX_TRANSCRIPT_MALFORMED
    #[error("no speech")]              Empty,                  // TRX_TRANSCRIPT_EMPTY
    #[error("audio undecodable")]      AudioUndecodable,       // TRX_AUDIO_UNDECODABLE
    #[error("model missing")]          ModelMissing,           // TRX_MODEL_MISSING
    #[error("download failed")]        DownloadFailed,         // TRX_MODEL_DOWNLOAD_FAILED, retryable
    #[error("checksum mismatch")]      ChecksumMismatch,       // TRX_MODEL_CHECKSUM, retryable
    #[error("no space")]               NoSpace,                // TRX_NO_SPACE
    #[error("cancelled")]              Cancelled,              // TRX_CANCELLED
    #[error("engine failed: {0}")]     Engine(i32),            // TRX_ENGINE_FAILED, retryable
    #[error("busy")]                   Busy,                   // TRX_BUSY, retryable
}
impl ordin_types::CodedError for TranscriptionError { /* code() per comment; retryable() true for
    DownloadFailed, ChecksumMismatch, Engine, Busy; repair() Some(TryAgain) for those four, else None */ }
```

### Bundled model list (`speech-models.json`)

```json
{"format":1,"models":[
 {"choice":"larger","file_name":"ggml-medium.bin","size_mb":1533,"sha256":"<64 hex>","url":"https://<model host>/ggml-medium.bin"},
 {"choice":"smaller","file_name":"ggml-small.bin","size_mb":488,"sha256":"<64 hex>","url":"https://<model host>/ggml-small.bin"}]}
```

`url` is `https` only; `sha256` 64 lower-case hex characters; `size_mb` 1–4,000. The file's values are filled from the whisper.cpp project's published model files when the crate is built and checked by C-109 (§7.8). A parse failure of this bundled file fails the crate's own unit test, so it cannot ship.

### Who calls each public item

| Item | Called by |
|---|---|
| `new` | desktop program, with `minutes` |
| `models`, `download_model`, `remove_model`, `read_transcript`, `transcribe_audio` | Minutes Service (MinutesService.md) only |

### Example

```json
{"format":1,"source_kind":"recording","source_name":"2026-01-14 10-02-11.mkv","language":"en",
 "duration_ms":3600000,"utterances":[
  {"start_ms":0,"end_ms":4200,"speaker":"You","text":"Thanks for joining. First item is the supplier renewal."},
  {"start_ms":4300,"end_ms":9100,"speaker":"Others","text":"We agreed to go with Northwind for another year."}]}
```

## Internal Logic

### A. Reading a transcript file (`read_transcript`)

1. Extension (lower-case) not `vtt`, `docx` or `txt` → `FormatUnsupported`. Size over `MAX_TRANSCRIPT_FILE_BYTES` → `FileTooLarge`. Open or read failure → `FileUnreadable`.
2. **`.vtt`**: first non-empty line must start with `WEBVTT` (else `TranscriptMalformed`). Blocks separated by blank lines; a cue block has an optional identifier line, a timing line `HH:MM:SS.mmm --> HH:MM:SS.mmm` (hours optional), then text lines. Speaker: a `<v Name>` voice tag (closing `</v>` optional; other tags removed), else a leading `Name: ` where `Name` is 1–80 characters with no `:`. `NOTE`, `STYLE` and `REGION` blocks are skipped. Consecutive cues of the same speaker are kept as separate utterances.
3. **`.docx`**: open as ZIP, read `word/document.xml` with `quick-xml`; each `w:p` becomes one line of its `w:t` texts joined. The lines are parsed by step 4's grammar. A missing `word/document.xml` or invalid XML → `TranscriptMalformed`.
4. **`.txt`** (UTF-8; a UTF-8 BOM removed; invalid UTF-8 → `TranscriptMalformed`) and `.docx` lines: a line matching a time `(\d{1,2}:)?\d{1,2}:\d{2}(\.\d{1,3})?` optionally followed by ` --> ` and a second time sets the start (and end) of the next utterance; a line that is a speaker name alone (1–80 characters, followed by a time on the same line or the next line) sets the speaker; any other non-empty line is utterance text, with a leading `Name: ` taken as speaker. An utterance with no end ends at the next start, the last at its start + 5,000 ms.
5. **Normalisation** (every source): text trimmed, runs of white space to one space, an utterance with empty text dropped; speaker trimmed; utterances sorted by `start_ms`. No utterance left → `Empty`. `duration_ms` = last `end_ms`. The same meeting given as `.vtt`, `.docx` and `.txt` gives text-equal transcripts after this step when speaker names and times are equal (criterion 2 of the request).
6. Log `transcription.read` (`info`: `kind`, `utterances`, `duration_ms`). No text is logged.

### B. Downloading a model (`download_model`)

1. Find `choice` in the bundled list. Model already present → return its info.
2. Free space in the models folder below `size_mb` + 100 MB → `NoSpace`.
3. GET `url` with `http` (no cookies, no credentials, no Ordin identifier beyond the client's generic user agent), streaming to `<file_name>.part`, sending `DownloadProgress` each 1 MB; a read idle for `DOWNLOAD_TIMEOUT_S` or a network error is retried `DOWNLOAD_RETRIES` times with `Range` resume; exhausted → delete the part file, `DownloadFailed`. Cancel → delete the part file, `Cancelled`.
4. Compute SHA-256 while writing; mismatch → delete the part file, `ChecksumMismatch`. Match → `fsync`, rename to `file_name`. Log `transcription.model_downloaded` (`info`: `choice`, `size_mb`, `duration_ms`).
5. One download at a time: a second call while one runs → `Busy`.

### C. Transcribing audio (`transcribe_audio`)

1. Extension not `m4a`, `mp3`, `wav` or `mkv` → `FormatUnsupported`; over `MAX_AUDIO_FILE_BYTES` → `FileTooLarge`; the model file of `model` absent → `ModelMissing`; another transcription running → `Busy` (one at a time).
2. Probe with `symphonia`; no audio track or an unsupported codec → `AudioUndecodable`; duration above `MAX_AUDIO_DURATION_MS` → `FileTooLarge`.
3. Decode on a blocking thread. `Mixed`: every audio track's channels mixed to mono. `YouAndOthers`: track 1 and track 2 each mixed to mono separately (a recording with one audio track is treated as `Mixed`). Resample to `SAMPLE_RATE_HZ`. Send `Decoding` progress every `PROGRESS_INTERVAL_MS`. Decoded audio is held in memory only (16 kHz mono `f32`: 230 MB per hour per track) and never written to disk.
4. Load the model with `whisper-rs` (Metal on Apple silicon, CPU with all performance cores elsewhere). Load failure → `Engine(code)`.
5. Run full recognition per track with language detection on, timestamps on, a progress callback sending `Recognising`, and an abort callback that returns true once `cancel` fires; whisper.cpp checks it between segments, so work stops within `CANCEL_DEADLINE_MS`. Cancelled → `Cancelled`; a non-zero result → `Engine(code)`.
6. Each segment becomes an `Utterance` with its times; `YouAndOthers` sets speaker "You" or "Others" by track (these two words are fixed data, not translated); `Mixed` sets none. Merge both tracks by `start_ms`. Normalise as A5. `language` from the first track's detection.
7. Free the model and the audio buffers. Log `transcription.done` (`info`: `source_kind`, `model`, `duration_ms`, `elapsed_ms`, `utterances`).

## Data Model

N/A — no database table. Files owned: the model files in the models folder (deleted by `remove_model`); `.part` files exist only during a download and are deleted on failure, cancel or at the next `new` (stale part files from a crash).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| File type not one of the six | `Err(permanent)` | `TRX_FORMAT_UNSUPPORTED` | `{"code":"TRX_FORMAT_UNSUPPORTED","message":"Ordin can make minutes from .vtt, .docx and .txt transcripts and from .m4a, .mp3 and .wav recordings.","action":null}` |
| File cannot be opened or read | `Err(permanent)` | `TRX_FILE_UNREADABLE` | `{"code":"TRX_FILE_UNREADABLE","message":"Ordin couldn't open that file. Check it is still there and try again.","action":null}` |
| Over 20 MiB (transcript), 4 GiB or 4 hours (audio) | `Err(permanent)` | `TRX_FILE_TOO_LARGE` | `{"code":"TRX_FILE_TOO_LARGE","message":"That file is too long. Ordin makes minutes of meetings up to four hours.","action":null}` |
| Not WebVTT, not a Word document, or not UTF-8 text | `Err(permanent)` | `TRX_TRANSCRIPT_MALFORMED` | `{"code":"TRX_TRANSCRIPT_MALFORMED","message":"Ordin couldn't read the transcript in that file.","action":null}` |
| No words found | `Err(permanent)` | `TRX_TRANSCRIPT_EMPTY` | `{"code":"TRX_TRANSCRIPT_EMPTY","message":"Ordin found no speech in that file.","action":null}` |
| No audio track or codec not supported | `Err(permanent)` | `TRX_AUDIO_UNDECODABLE` | `{"code":"TRX_AUDIO_UNDECODABLE","message":"Ordin couldn't play the sound in that file.","action":null}` |
| Chosen speech model not on the computer | `Err(permanent)` | `TRX_MODEL_MISSING` | `{"code":"TRX_MODEL_MISSING","message":"Ordin needs to download its speech model first.","action":null}` |
| Download failed after retries | `Err(retryable)` | `TRX_MODEL_DOWNLOAD_FAILED` | `{"code":"TRX_MODEL_DOWNLOAD_FAILED","message":"The speech model didn't download. Check your connection and try again.","action":"try_again"}` |
| Downloaded file differs from the expected checksum | `Err(retryable)` | `TRX_MODEL_CHECKSUM` | `{"code":"TRX_MODEL_CHECKSUM","message":"The speech model arrived damaged and was removed. Try again.","action":"try_again"}` |
| Not enough free space | `Err(permanent)` | `TRX_NO_SPACE` | `{"code":"TRX_NO_SPACE","message":"There isn't enough free space for the speech model. Free some space, then try again.","action":null}` |
| Cancelled by the person | `Err(permanent)` | `TRX_CANCELLED` | `{"code":"TRX_CANCELLED","message":"Stopped.","action":null}` |
| Model failed to load or recognition failed | `Err(retryable)` | `TRX_ENGINE_FAILED` | `{"code":"TRX_ENGINE_FAILED","message":"Ordin couldn't turn the recording into text. Try again.","action":"try_again"}` |
| A download or transcription already running | `Err(retryable)` | `TRX_BUSY` | `{"code":"TRX_BUSY","message":"Ordin is already working on another recording. It will start this one after.","action":"try_again"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Transcription Service

  Scenario: Happy path — a WebVTT transcript keeps its speakers
    Given the committed fixture "meeting.vtt" with voice tags for "Sam Lee" and "Priya Shah"
    When read_transcript is called
    Then the transcript has source_kind "transcript_file", no language, and each utterance carries its speaker

  Scenario: Edge — the same meeting as .vtt, .docx and .txt
    Given the fixtures "meeting.vtt", "meeting.docx" and "meeting.txt" of the same meeting
    When each is read
    Then the three utterance lists are equal in speaker and text after normalisation

  Scenario: Happy path — a recording of 60 seconds becomes text
    Given the larger model is present and the fixtures "speech-60s.m4a", ".mp3" and ".wav"
    When transcribe_audio is called with Mixed for each
    Then each transcript has no speaker labels and a word error rate of at most 20 % against "speech-60s.txt"

  Scenario: Edge — an Ordin recording is labelled You and Others
    Given the fixture "two-track.mkv" with speech on track 1 and on track 2
    When transcribe_audio is called with YouAndOthers
    Then utterances from track 1 have speaker "You" and from track 2 "Others", merged by start time

  Scenario: Error — no model yet
    Given no model file is present
    When transcribe_audio is called
    Then the error is TRX_MODEL_MISSING and the models folder stays empty

  Scenario: Error — damaged download
    Given a test model host serving a file whose checksum differs
    When download_model is called
    Then the error is TRX_MODEL_CHECKSUM and neither the model file nor a part file remains

  Scenario: Edge — cancel stops within five seconds
    Given a 60-minute recording is being recognised
    When cancel fires
    Then transcribe_audio returns TRX_CANCELLED within 5 seconds

  Scenario: Edge — no audio leaves the computer
    Given networking is blocked except the model host
    When a recording is transcribed with the model present
    Then no outbound connection is opened

  Scenario: Error — an unsupported file
    When read_transcript is called with "notes.pdf"
    Then the error is TRX_FORMAT_UNSUPPORTED
```

## Performance, Security, Observability

- **Performance targets** (reference machines, SA-MIN-03): a 60-minute recording with the larger model within 20 minutes on the Apple-silicon reference machine and within 45 minutes on the x86 reference machine; with the smaller model within 8 and 15 minutes. `read_transcript` of a 20 MiB file within 2 s p95. Progress at least every second; cancel within 5 s. Peak memory with the larger model and one hour of two-track audio: ≤ 2.5 GiB. Conditional on C-110 (§7.8).
- **Security:** no audio, transcript text or file path content is logged or sent; the download sends no credential and goes only to the `https` address of the bundled list; the model is used only after its SHA-256 matched; files are opened read-only; decoded audio is never written to disk; inputs are bounded (sizes, durations, line and utterance lengths).
- **Observability** (`tracing` JSON lines, `component = "transcription"`): `transcription.read` (info: `kind`, `utterances`, `duration_ms`), `transcription.done` (info: `source_kind`, `model`, `duration_ms`, `elapsed_ms`, `utterances`), `transcription.model_downloaded` (info: `choice`, `size_mb`, `duration_ms`), `transcription.download_failed` (warn: `choice`, `attempts`), `transcription.failed` (warn: `code`). Never logged: text, speaker names, file names, paths.

## Gaps

None. The model list's addresses and checksums, the binding's licence and the time budgets are bounded by confirmation items C-108 to C-110 (Generation Playbook §7.8), each with a stated fallback.

## Amendment 2026-11-21 — Recordings Ordin asks to have turned into text (TRX-ORD-1, TRX-ORD-2)

> **Status:** Approved by the user, 2026-11-21 ("approve as written"). Every section above stays in force. Source: the user's decision of 2026-11-20 that Ordin may turn any recording into text, on this computer, after the person's consent to the model.

- **TRX-ORD-1 Still one caller.** The service is still called only by the Minutes Service. Ordin's `transcribe_recording` (OrdinHost.md OH-MIN-7) reaches it as a Minutes Service job of purpose `transcript` (MinutesService.md MIN-TR-1), so the model consent, the one-at-a-time rule (`Busy`) and cancellation are the ones above, unchanged. No interface of this service changes.
- **TRX-ORD-2 Any Matroska file.** `.mkv` is accepted from any folder, not only from Ordin's own recordings; a Matroska file with more than one audio track is read as one mixed track unless the caller passes `YouAndOthers` (only Ordin's own recordings do). The *Purpose* line "the `.mkv` files of Ordin's own recordings" reads "`.mkv` files" from this amendment on. The service still sends no audio or text anywhere: the text leaves the computer only when Ordin reads it (the one-time notice, MinutesService.md §B6).

## Amendment 2026-11-22 — The speech model chosen by the machine (TRX-MACH-1 to TRX-MACH-4)

> **Status:** Approved by the user, 2026-11-22 ("approve as written"). Every section above stays in force except where this section replaces it. Source: the user's decision of 2026-11-22 that Ordin recommends the larger or the smaller model from the computer it runs on (plan 07 FU-022). Evidence: on an 8-processor x86 machine (Ryzen 9 9950X, Windows 11, an optimised build) the larger model took 45.7 s per 60 s of audio, about 46 minutes per hour of meeting — at the x86 budget of SA-MIN-03 — so a machine with fewer processors would miss it.

- **TRX-MACH-1 Replaces Shared Context "Default model".** There is no fixed default. The Minutes Service uses the model `recommended_model()` names unless the person chose one (MinutesService.md MIN-MACH-1). `SpeechModelChoice` keeps its two values; the comment `// default Larger` no longer applies.
- **TRX-MACH-2 What the computer is.** `pub struct MachineFacts { pub apple_silicon: bool, pub logical_processors: u32, pub memory_gib: u32, pub avx2: bool }` and `pub fn machine_facts() -> MachineFacts`, read without the network and without a permission prompt:
  - `apple_silicon`: `target_os = "macos"` and `target_arch = "aarch64"`.
  - `logical_processors`: `std::thread::available_parallelism()`; unreadable → 1.
  - `memory_gib`: total physical memory in whole GiB, rounded down — macOS `sysctlbyname("hw.memsize")` through `libc`; Windows `GlobalMemoryStatusEx` (`ullTotalPhys`) through `windows-sys` with the feature `Win32_System_SystemInformation`; unreadable → 0.
  - `avx2`: `is_x86_feature_detected!("avx2")` on `x86_64`; `false` elsewhere.
- **TRX-MACH-3 The rule.** `pub fn recommend(facts: &MachineFacts) -> SpeechModelChoice` gives `Larger` when either (a) `apple_silicon` and `memory_gib` ≥ 15, or (b) `avx2` and `logical_processors` ≥ 8 and `memory_gib` ≥ 15; otherwise `Smaller`. 15, not 16: a 16 GB computer reports slightly less than 16 GiB of physical memory. `pub fn recommended_model() -> SpeechModelChoice` is `recommend(&machine_facts())`, computed once per process and kept. It logs `transcription.model_recommended` (info: `choice`, `apple_silicon`, `logical_processors`, `memory_gib`, `avx2`) once. No interface above changes; `download_model`, `remove_model` and `transcribe_audio` still take an explicit `SpeechModelChoice`.
- **TRX-MACH-4 Errors.** None new: the facts never fail; an unreadable value falls to the value that recommends `Smaller`.

```gherkin
Feature: The speech model chosen by the machine (TRX-MACH-1 to TRX-MACH-4)

  Scenario Outline: The recommendation follows the machine
    Given the machine facts apple_silicon <apple>, logical_processors <cpus>, memory_gib <mem>, avx2 <avx2>
    When recommend is called
    Then the answer is <choice>

    Examples:
      | apple | cpus | mem | avx2  | choice  |
      | true  | 8    | 16  | false | Larger  |
      | true  | 8    | 8   | false | Smaller |
      | false | 16   | 32  | true  | Larger  |
      | false | 8    | 15  | true  | Larger  |
      | false | 4    | 16  | true  | Smaller |
      | false | 16   | 8   | true  | Smaller |
      | false | 16   | 32  | false | Smaller |
      | false | 16   | 0   | true  | Smaller |

  Scenario: Reading the machine needs nothing from outside
    Given networking is blocked
    When machine_facts is called
    Then it returns within 100 ms and logical_processors is at least 1
