# SPEC: OBS Recorder Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** added (amended January 2026; user decision; Approved by the user January 2026; amended November 2026: `RecordingStarted.record_folder`, user decision; amended 2026-11-22: OBS-PRO-1 to OBS-PRO-3 and OBS-STOP-1, user decision) — meeting minutes from transcripts, recordings and OBS
> **Mode:** greenfield
> **derivedFromHld:** 0.72.0
> **Crate:** `ordin-obs` · **Build phase:** 3 · **Programs:** D · **Build switch:** `minutes`
> **Dependencies:** Identity and Credentials, Localisation

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

The OBS Recorder Adapter lets Ordin record a meeting's sound with OBS, the free recording program the person installs themselves. It finds whether OBS is installed, talks to OBS's built-in WebSocket server (OBS 28 or later, obs-websocket protocol version 5) on this computer, prepares a sound-only scene named **Ordin meeting** with the microphone as **You** on track 1 and the computer's sound as **Others** on track 2, and starts and stops a recording. It returns the finished file's path to the Minutes Service. It never installs OBS, never changes the person's own OBS scenes or profile, and keeps the WebSocket password only in the secure credential store through Identity and Credentials.

## Approach

One crate holding a small obs-websocket v5 client over the workspace's `tokio-tungstenite` (already in `Cargo.lock`), with one connection opened per operation and closed after it, so no long-lived socket can go stale while OBS restarts. Recording settings Ordin needs (advanced output, two tracks, Matroska file) are applied to a dedicated OBS profile **Ordin meeting** that the adapter selects only while recording and switches back from at Stop. Rejected: *an entry in the Craft Toolkit's `craft-tools.json`* — that list is for apps the organisation approves, downloads and verifies, with MCP tools, and OBS is neither downloaded nor driven by an agent; *driving OBS's command line* — it cannot stop a recording or report the output path; *changing the person's current OBS profile* — it would change their own recordings.

## Shared Context

### Facts this component implements

- OBS is detected, not installed. The download page shown to the person is `https://obsproject.com/download`.
- OBS version 28 or later, obs-websocket protocol version 5 (RPC version 1), on `127.0.0.1` at the port in the minutes settings (default 4455).
- The WebSocket password is stored and read only through `ordin-identity` (IdentityAndCredentials.md ID-OBS-1); it appears in no log line, settings file or journal row.
- Sound only: the scene holds no video source, so the recorded picture is the empty canvas.
- System sound: on Windows the output capture input; on macOS the screen-capture sound input, which OBS has from version 30. With OBS 28 or 29 on macOS the scene holds the microphone only, and the adapter reports `others_heard: false`.

### Identity interface consumed (ID-OBS-1)

```rust
impl IdentityService {
    pub async fn obs_password(&self) -> Result<Option<SecretString>, CredError>;
}
```

### Names chosen here

| Name | Kind |
|---|---|
| `ObsRecorder`, `ObsState`, `ObsStatus`, `RecordingStarted`, `RecordingFinished`, `ObsError` | public types |
| scene **Ordin meeting**, inputs **You** and **Others**, profile **Ordin meeting** | OBS objects created by Ordin (fixed data, not translated) |
| `OBS_*` | error codes (Error Table) |
| `errors-obs.ftl` | en-GB catalogue of the `OBS_` sentences |

## Public Interface

```rust
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ObsState {
    NotInstalled,
    /// Installed, but nothing answers on the port (OBS closed, or its WebSocket server off).
    NotReachable,
    /// The server asks for a password and none is stored.
    PasswordNeeded,
    /// The stored password was refused.
    PasswordRefused,
    /// OBS older than 28 or a WebSocket RPC version other than 1.
    VersionUnsupported { obs_version: String },
    Ready { obs_version: String, others_heard: bool },
    Recording { started_at: Timestamp, others_heard: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordingStarted {
    pub started_at: Timestamp,
    pub others_heard: bool,
    /// OBS's recording folder (`GetRecordDirectory`) read before StartRecord; the Minutes Service
    /// keeps it to find the file when OBS is closed during the recording (MinutesService.md §D4).
    /// Never logged and never sent to the screens.
    pub record_folder: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordingFinished { pub output_path: PathBuf, pub duration_ms: u64 }

pub struct ObsRecorder { /* private: Arc<IdentityService>, Arc<Localisation>, port, current-profile memory */ }

impl ObsRecorder {
    pub fn new(identity: Arc<IdentityService>, localisation: Arc<Localisation>) -> Arc<Self>;
    /// Port 1024–65535; default 4455. Takes effect at the next call.
    pub fn set_port(&self, port: u16) -> Result<(), ObsError>;
    /// Detects the installation and, when installed, connects and identifies; never starts OBS.
    pub async fn state(&self) -> ObsState;
    /// Starts OBS (macOS: `open -a <bundle>`; Windows: the detected `obs64.exe`, working directory its folder).
    pub async fn open_obs(&self) -> Result<(), ObsError>;
    /// Ensures the profile and scene, reads the recording folder, then sends StartRecord.
    pub async fn start(&self) -> Result<RecordingStarted, ObsError>;
    /// Sends StopRecord, restores the person's profile and scene, returns the file.
    pub async fn stop(&self) -> Result<RecordingFinished, ObsError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ObsError {
    #[error("not installed")]          NotInstalled,         // OBS_NOT_INSTALLED
    #[error("not reachable")]          NotReachable,         // OBS_NOT_REACHABLE, retryable
    #[error("password needed")]        PasswordNeeded,       // OBS_PASSWORD_NEEDED
    #[error("password refused")]       PasswordRefused,      // OBS_PASSWORD_REFUSED
    #[error("version unsupported")]    VersionUnsupported,   // OBS_VERSION_UNSUPPORTED
    #[error("already recording")]      AlreadyRecording,     // OBS_ALREADY_RECORDING
    #[error("not recording")]          NotRecording,         // OBS_NOT_RECORDING
    #[error("request failed: {0}")]    Request(u16),         // OBS_REQUEST_FAILED, retryable
    #[error("no answer in time")]      Timeout,              // OBS_TIMEOUT, retryable
    #[error("scene not prepared")]     SceneFailed,          // OBS_SCENE_FAILED, retryable
    #[error("output missing")]         OutputMissing,        // OBS_OUTPUT_MISSING
    #[error("port invalid")]           PortInvalid,          // OBS_PORT_INVALID
}
impl ordin_types::CodedError for ObsError { /* retryable() true for NotReachable, Request, Timeout,
    SceneFailed; repair() Some(TryAgain) for those four, else None */ }
```

### Who calls each public item

| Item | Called by |
|---|---|
| `new`, `set_port` | desktop program, with `minutes` (port from the minutes settings at start and on change) |
| `state`, `open_obs`, `start`, `stop` | Minutes Service (MinutesService.md §D) |

### Example (WebSocket frames, abridged)

```json
← {"op":0,"d":{"obsWebSocketVersion":"5.4.2","rpcVersion":1,"authentication":{"challenge":"+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=","salt":"lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI="}}}
→ {"op":1,"d":{"rpcVersion":1,"authentication":"1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4=","eventSubscriptions":0}}
← {"op":2,"d":{"negotiatedRpcVersion":1}}
→ {"op":6,"d":{"requestType":"StartRecord","requestId":"0192b3c4-2222-7000-8000-000000000001"}}
← {"op":7,"d":{"requestType":"StartRecord","requestId":"0192b3c4-2222-7000-8000-000000000001","requestStatus":{"result":true,"code":100}}}
```

## Internal Logic

### A. Detection

1. macOS: `/Applications/OBS.app` or `~/Applications/OBS.app` exists. Windows: the default value of registry key `HKLM\SOFTWARE\OBS Studio` names a folder holding `bin\64bit\obs64.exe`, else `%ProgramFiles%\obs-studio\bin\64bit\obs64.exe` exists. Neither → `NotInstalled`. Other targets → `NotInstalled`.

### B. Connecting (every operation)

1. Connect `ws://127.0.0.1:<port>` with subprotocol `obswebsocket.json`, timeout 3 s; refused or timed out → `NotReachable`.
2. Read `Hello` (op 0) within 3 s. `rpcVersion` ≠ 1 → `VersionUnsupported`. With `authentication`: read the password through `obs_password()`; none → `PasswordNeeded`; else `secret = base64(sha256(password + salt))`, `auth = base64(sha256(secret + challenge))`. Send `Identify` (op 1) with `eventSubscriptions: 0`. Close code 4009 → `PasswordRefused`. `Identified` (op 2) within 3 s, else `Timeout`.
3. Requests (op 6) carry a UUIDv7 `requestId`; the answer (op 7) must arrive within 5 s (`StopRecord` 15 s), else `Timeout`; `requestStatus.result = false` → `Request(code)` unless a step below names the code.
4. `GetVersion` once per connection: `obsVersion` below 28.0.0 → `VersionUnsupported`. `others_heard` = Windows, or macOS with `obsVersion` ≥ 30.0.0.

### C. Preparing (in `start`, before StartRecord)

1. `GetRecordStatus`: `outputActive` true → `AlreadyRecording`.
2. Remember the current profile (`GetProfileList.currentProfileName`) and scene (`GetCurrentProgramScene.sceneName`) in memory.
3. Profile **Ordin meeting** absent from `GetProfileList` → `CreateProfile`. `SetCurrentProfile` to it; then `SetProfileParameter` for `Output/Mode = Advanced`, `AdvOut/RecType = Standard`, `AdvOut/RecFormat2 = mkv`, `AdvOut/RecTracks = 3`, `AdvOut/Track1Name = You`, `AdvOut/Track2Name = Others` (conditional on C-111, §7.8; fallback: the recording is made with OBS's current settings, the transcript is `Mixed` and the Minutes Service is told by `others_heard: false`).
4. Scene **Ordin meeting** absent from `GetSceneList` → `CreateScene`, then `CreateInput` **You** (`coreaudio_input_capture` on macOS, `wasapi_input_capture` on Windows; default device) and, when `others_heard`, **Others** (`sck_audio_capture` on macOS, `wasapi_output_capture` on Windows; default device); `SetInputAudioTracks` **You** → `{"1":true,"2":false,…}`, **Others** → `{"1":false,"2":true,…}`. A present scene is left as it is (the person may have chosen other devices in it). Any failure in steps 3–4 → restore step 2's profile and scene, `SceneFailed`.
5. `SetCurrentProgramScene` **Ordin meeting**.
6. `GetRecordDirectory` → `responseData.recordDirectory`, read after step 3 so it is the folder of the **Ordin meeting** profile. Missing or empty → `Request(0)`; any failure → restore step 2's profile and scene and return the error.

### D. Start and stop

1. `start`: C1–C6, then `StartRecord`. Answer → `RecordingStarted { started_at: now, others_heard, record_folder }`; log `obs.recording_started` (info: `obs_version`, `others_heard`). Failure → restore the profile and scene, return the error.
2. `stop`: `GetRecordStatus.outputActive` false → `NotRecording`. `StopRecord` → `responseData.outputPath`. Then `SetCurrentProfile` and `SetCurrentProgramScene` back to the remembered values (failures logged as `obs.restore_failed` warn, not returned). The path must exist and be a file within 10 s (OBS finishes writing), else `OutputMissing`. Result `RecordingFinished { output_path, duration_ms }` from `GetRecordStatus.outputDuration` read before the stop. Log `obs.recording_stopped` (info: `duration_ms`).
3. If OBS was closed during the recording, `stop` answers `NotReachable`; the Minutes Service then looks for the newest `.mkv` in `RecordingStarted.record_folder` modified after `started_at` (MinutesService.md §D4).

## Data Model

N/A — no table, no file. The profile and scene **Ordin meeting** live in OBS's own configuration; the remembered profile and scene names live in memory only.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| OBS not installed | `Err(permanent)` | `OBS_NOT_INSTALLED` | `{"code":"OBS_NOT_INSTALLED","message":"To record meetings, install OBS, the free recording program, then come back here.","action":null}` |
| Nothing answers on the port | `Err(retryable)` | `OBS_NOT_REACHABLE` | `{"code":"OBS_NOT_REACHABLE","message":"Open OBS, then choose Tools › WebSocket Server Settings and tick Enable WebSocket server.","action":"try_again"}` |
| OBS asks for a password and none is saved | `Err(permanent)` | `OBS_PASSWORD_NEEDED` | `{"code":"OBS_PASSWORD_NEEDED","message":"Paste the OBS password: in OBS choose Tools › WebSocket Server Settings › Show Connect Info and copy the Server Password.","action":null}` |
| The saved password was refused | `Err(permanent)` | `OBS_PASSWORD_REFUSED` | `{"code":"OBS_PASSWORD_REFUSED","message":"OBS didn't accept the password. Copy it again from Tools › WebSocket Server Settings › Show Connect Info.","action":null}` |
| OBS older than 28, or another protocol | `Err(permanent)` | `OBS_VERSION_UNSUPPORTED` | `{"code":"OBS_VERSION_UNSUPPORTED","message":"Ordin needs OBS 28 or later. Update OBS, then try again.","action":null}` |
| OBS is already recording | `Err(permanent)` | `OBS_ALREADY_RECORDING` | `{"code":"OBS_ALREADY_RECORDING","message":"OBS is already recording. Stop that recording in OBS first.","action":null}` |
| Stop with no recording running | `Err(permanent)` | `OBS_NOT_RECORDING` | `{"code":"OBS_NOT_RECORDING","message":"OBS isn't recording.","action":null}` |
| OBS refused a request | `Err(retryable)` | `OBS_REQUEST_FAILED` | `{"code":"OBS_REQUEST_FAILED","message":"OBS didn't do what Ordin asked. Try again.","action":"try_again"}` |
| No answer within the limit | `Err(retryable)` | `OBS_TIMEOUT` | `{"code":"OBS_TIMEOUT","message":"OBS didn't answer. Try again.","action":"try_again"}` |
| Profile or scene could not be prepared | `Err(retryable)` | `OBS_SCENE_FAILED` | `{"code":"OBS_SCENE_FAILED","message":"Ordin couldn't set up its meeting scene in OBS. Try again.","action":"try_again"}` |
| The recorded file is not where OBS said | `Err(permanent)` | `OBS_OUTPUT_MISSING` | `{"code":"OBS_OUTPUT_MISSING","message":"Ordin couldn't find the recording OBS made.","action":null}` |
| Port outside 1024–65535 | `Err(permanent)` | `OBS_PORT_INVALID` | `{"code":"OBS_PORT_INVALID","message":"Choose a port number from 1024 to 65535.","action":null}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: OBS Recorder Adapter

  Scenario: Happy path — record and stop against a fake OBS
    Given a fake obs-websocket v5 server reporting OBS 30.2.0 with password "fixture-pass" saved
    When start is called
    Then the server receives Identify with the correct authentication string, then StartRecord
    When stop is called
    Then the server receives StopRecord and the result's output_path is the path the server returned

  Scenario: Edge — start reports OBS's recording folder
    Given the fake server's recording folder is "/fixture/recordings"
    When start is called
    Then GetRecordDirectory is sent before StartRecord and record_folder is "/fixture/recordings"

  Scenario: Edge — the meeting scene is created only when absent
    Given the fake server has no scene "Ordin meeting"
    When start is called twice with a stop between
    Then CreateScene, CreateInput "You" and CreateInput "Others" are each sent once
    And "You" is on track 1 only and "Others" on track 2 only

  Scenario: Edge — the person's profile and scene come back
    Given OBS's current profile is "Streaming" and scene "Camera"
    When a recording is started and stopped
    Then the current profile is "Streaming" and the scene "Camera" again

  Scenario: Error — three distinct failures
    Given OBS is not installed, or the port refuses, or the password is refused
    When state is called
    Then it answers not_installed, not_reachable or password_refused respectively

  Scenario: Edge — older OBS on macOS records the microphone only
    Given macOS and a fake server reporting OBS 29.1.3
    When start is called
    Then only the input "You" is created and others_heard is false

  Scenario: Edge — the password never appears
    When the tests run with password "fixture-pass"
    Then no log line and no file under the data folder contains "fixture-pass"
```

## Performance, Security, Observability

- **Performance targets:** `state` within 4 s worst case (3 s connect limit), p95 ≤ 150 ms with OBS running; `start` p95 ≤ 1.5 s including scene creation; `stop` p95 ≤ 3 s.
- **Security:** connects to `127.0.0.1` only, never to another host; the password is read from Identity per connection, used for the hash, and dropped (`SecretString`, zeroised); the `Hello` salt, challenge and the computed string are never logged; inputs bounded (port range, frame size ≤ 1 MiB, answers parsed with `deny_unknown_fields` off but every used field checked for type).
- **Observability** (`component = "obs"`): `obs.state` (debug: `state`), `obs.recording_started` (info: `obs_version`, `others_heard`), `obs.recording_stopped` (info: `duration_ms`), `obs.restore_failed` (warn: `what`), `obs.profile_not_reloaded` (warn, OBS-PRO-2), `obs.failed` (warn: `code`, `request_type`). Never logged: password, authentication strings, paths.

## Amendment 2026-11-22 — OBS loads Ordin's profile after its settings are written (OBS-PRO-1 to OBS-PRO-3)

> **Status:** Approved by the user, 2026-11-22 ("fix it, amend the spec"). Every section above stays in force except where this section replaces it. Source: the Phase 12 smoke test of plan 07 on macOS with OBS 33.0.1. The **Ordin meeting** profile was created and selected, then given `Output/Mode = Advanced` and `AdvOut/RecFormat2 = mkv`; its `basic.ini` held them, yet OBS recorded `hybrid_mov`, the *Simple* mode format of a new profile. OBS builds its recording output from a profile's settings when the profile is selected; `SetProfileParameter` writes the file and does not rebuild it.

- **OBS-PRO-1 Replaces C3's order.** After the six `SetProfileParameter` requests of C3, the adapter selects **Ordin meeting** again so that OBS builds its recording output from them: `SetCurrentProfile` to a *partner* profile, then `SetCurrentProfile` to **Ordin meeting**. The partner is the profile remembered in C2 when that is not **Ordin meeting**; otherwise the first other name in `GetProfileList.profiles`, in list order. This runs on every `start`, not only when the profile was just created, because the person may have changed the profile's settings in OBS since the last recording.
- **OBS-PRO-2 No partner.** When **Ordin meeting** is the only profile, no reload is possible: the recording goes ahead with OBS's current settings and `obs.profile_not_reloaded` (warn) is logged. The fallback of C3 then applies to whatever OBS records: a file the Minutes Service cannot move is read where OBS wrote it (MinutesService.md D3), and a format the Transcription Service does not read fails the job with `TRX_FORMAT_UNSUPPORTED`.
- **OBS-PRO-3 Errors.** None new. A failed `SetCurrentProfile` in OBS-PRO-1 is a failure of C3: restore C2's profile and scene and answer `SceneFailed`, as C3 says for its own steps.

```gherkin
Feature: OBS loads Ordin's profile after its settings are written (OBS-PRO-1 to OBS-PRO-3)

  Scenario: The recording settings are in force when the recording starts
    Given OBS's current profile is "Untitled" and no profile "Ordin meeting" exists
    When start is called
    Then the profile "Ordin meeting" was selected after its six parameters were set
    And the parameters OBS loaded for "Ordin meeting" include AdvOut/RecFormat2 = mkv and Output/Mode = Advanced
    And after stop the current profile is "Untitled" again

  Scenario: The person was already on Ordin's profile
    Given OBS's current profile is "Ordin meeting" and the profiles are "Ordin meeting" and "Untitled"
    When start is called
    Then "Untitled" is selected and then "Ordin meeting", after the parameters were set

  Scenario: No other profile to switch through
    Given the only profile is "Ordin meeting"
    When start is called
    Then the recording starts and obs.profile_not_reloaded is logged
```

## Amendment 2026-11-22 — `stop` waits for OBS to finish the file (OBS-STOP-1)

> **Status:** Approved by the user, 2026-11-22 ("fix the stop timing"). Every section above stays in force except where this section replaces it. Source: the Phase 12 smoke test of plan 07 on macOS with OBS 33.0.1: the job failed with `TRX_AUDIO_UNDECODABLE` 2 ms after `stop`, while the same file, read a minute later, held a video track and two complete 48 kHz AAC tracks. OBS answers `StopRecord` with `outputPath` before it has closed the file.

- **OBS-STOP-1 Replaces D2's "The path must exist and be a file within 10 s".** After `StopRecord`, the file counts as finished when it is a file, its size is above 0 bytes, and its size has stayed the same across 1 s of reads every 100 ms. All of this must happen within 10 s of the `StopRecord` answer, else `OutputMissing`. No new error.

```gherkin
Feature: stop waits for OBS to finish the file (OBS-STOP-1)

  Scenario: Stop waits for OBS to finish the file
    Given OBS answers StopRecord while it is still writing the file
    When stop is called
    Then it answers only after the file's size has stayed the same for 1 s, with the complete file

  Scenario: A file that stays empty is missing
    Given the file OBS names stays at 0 bytes
    When stop is called
    Then it answers OutputMissing after 10 s
```

## Gaps

None. The profile parameters and input kinds are bounded by C-111 (Generation Playbook §7.8) with its fallback.
