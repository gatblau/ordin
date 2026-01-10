# SPEC: Dictation Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (`Availability` and `UnavailableReason` are named `DictationAvailability` and `DictationUnavailableReason`, apart from the Package Catalogue's `Availability` and the start-up report's `UnavailableReason`)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-dictation` · **Build phase:** 3 · **Programs:** D
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

The Dictation Service lets the person speak instead of typing in the conversation and in every text field of a form (ADR-043, 11 §5.12). While the person has dictation on, it captures microphone audio and turns it into text on the computer with Apple's `SpeechAnalyzer` and `SpeechTranscriber` (or `DictationTranscriber` where the locale needs it) through a Swift bridge, on macOS 26 and later only (ADR-078, 02:539). It returns text to the field the person is dictating into; it never sends a message, never approves anything, and keeps no audio (02:539–541). On Windows, macOS before 26 and the ordin server it reports dictation as unavailable, and the microphone button is not shown (ADR-078). It owns nothing persisted: from revision 0.22.0 there is no speech model (02:540).

## Approach

The crate is a thin Rust façade over a Swift static library compiled by `build.rs` with `swift build` and called through a C ABI (`@_cdecl` functions on the Swift side, `extern "C"` declarations in Rust), with recognised text flowing back through one C callback into a `tokio::sync::mpsc` channel per session. On every other target the same Rust API compiles to a stub that reports the platform as unavailable. One session at a time is allowed; the session's locale comes from Localisation. Rejected: *a Swift-bridging crate (`swift-rs` or `swift-bridge`)* — neither is named in 1A or 05, and four C functions need no generator; *`SFSpeechRecognizer`* — rejected by ADR-043 and ADR-078 (network service, one-minute limit).

## Shared Context

### HLD facts this component implements

- macOS 26 and later: `SpeechAnalyzer` + `SpeechTranscriber`, or `DictationTranscriber` where the locale needs it; Swift-only APIs reached through a small Swift bridge; on-device models the system installs through `AssetInventory` (08 — Apple Speech framework row; ADR-043).
- macOS before 26, Windows, the ordin server: dictation off; no bundled model on any platform (ADR-078).
- Dictated text is reviewed and sent by the person; a spoken "yes" is never an approval; audio never leaves the computer and is never stored; only the text the person sends enters the conversation and the journal (ADR-043, ADR-078).
- The dictation language follows the person's locale (ADR-042); a locale the transcriber does not support shows the microphone as unavailable with the reason (ADR-078).
- Permission for the microphone, and on macOS for speech recognition, is asked the first time the person uses dictation, with the purpose in plain words; the macOS build carries the usage descriptions and the audio-input entitlement (ADR-043 *Consequences*). Where device management has switched the permission off, dictation is off and ordin says so plainly (ADR-072).
- Words fixed by 11: Windows — "Dictation isn't available on Windows."; older macOS — "Dictation isn't available on this version of macOS." (11 §5.12, ADR-078).
- Dictation also works into a server's workspace shown on the desktop (11:1015): the recognition runs on the desktop; only the sent text crosses to the server.

### Localisation interface consumed

```rust
impl Localisation {
    pub fn active(&self) -> ActiveLocale;
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<ActiveLocale>;
    pub fn message(&self, id: MessageId, args: &MessageArgs) -> String;
}
pub struct ActiveLocale { pub locale: String, pub source: LocaleSource, pub direction: TextDirection, pub fallback_chain: Vec<String> }
```

### Service API conventions

Commands `<module>.<verb>` (SA-API-01); errors `ServiceError { code, message, action }` with `message` from Localisation (SA-ENV-01); `RepairAction` ∈ {`try_again`, `sign_in_again`, `send_to_it`}.

### Names chosen here

| Name | Kind | Reason |
|---|---|---|
| `crates/ordin-dictation/swift/` (Swift package `OrdinDictation`, product a static library) | source layout | the "small Swift bridge" of ADR-043 |
| C functions `ordin_dict_availability`, `ordin_dict_start`, `ordin_dict_stop`, `ordin_dict_cancel` and callback type `ordin_dict_callback` | bridge ABI | four calls cover availability, start, stop and cancel |
| `dictation.get_availability`, `dictation.start`, `dictation.stop`, `dictation.cancel` | Service API commands | SA-API-01 form |
| `DictationUpdate` delivered on the `dictation.start` command's IPC channel, not as journalled events | transport | dictated text must not enter the journal before the person sends it (ADR-043); see HLD-impact B2-F45 |
| Info.plist `NSMicrophoneUsageDescription`, `NSSpeechRecognitionUsageDescription`; entitlement `com.apple.security.device.audio-input` | Apple keys (Apple's names) | required by ADR-043 *Consequences* |
| Message ids `dictation-unavailable-windows`, `dictation-unavailable-macos-version`, `dictation-unavailable-language`, `dictation-unavailable-microphone`, `dictation-unavailable-speech`, `dictation-getting-ready`, `dictation-failed` | catalogue messages | words shown to the person, owned here |

### Reconciliation assumptions

- **SA-RECON-01** The Service API catalogue takes the four `dictation.*` commands as written (R1, R6); text reaches the screens only as `Partial` and `Final` updates on the command's channel (row 189).
- **SA-RECON-02** The Command Bridge passes one Tauri IPC channel through for `dictation.start` (row 190; the Bridge owns that mechanism).
- **SA-RECON-03** The desktop program cancels the running session on quit and on sleep (row 191).

## Public Interface

```rust
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DictationSessionId(pub uuid::Uuid); // UUIDv7, chosen here; never stored

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DictationAvailability {
    Available { locale: String },
    /// Locale supported, recognition assets not yet installed by the system; starting will request them.
    NeedsAssets { locale: String },
    Unavailable { reason: DictationUnavailableReason, /** plain sentence in the person's locale */ message: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DictationUnavailableReason {
    Windows,
    MacosTooOld,
    /// Built for a target with no dictation at all (the ordin server).
    NotOffered,
    LanguageNotSupported,
    MicrophoneDenied,
    SpeechRecognitionDenied,
}

/// Where the text goes; opaque to this crate, chosen by the screens (≤ 128 chars).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct FieldRef(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DictationUpdate {
    /// Assets are being installed by the system; the field shows "Getting dictation ready…".
    Preparing { session_id: DictationSessionId },
    /// Listening has begun.
    Listening { session_id: DictationSessionId },
    /// Replaces the provisional tail of the field's dictated text (volatile result).
    Partial { session_id: DictationSessionId, text: String },
    /// Final text for a phrase; appended to the field and no longer replaced.
    Final { session_id: DictationSessionId, text: String },
    /// The session has ended; no further update follows.
    Ended { session_id: DictationSessionId, reason: EndReason },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndReason { Stopped, Cancelled, Replaced, Silence, TimeLimit, LocaleChanged, Failed }

pub struct DictationSession {
    pub session_id: DictationSessionId,
    pub field: FieldRef,
    pub updates: mpsc::Receiver<DictationUpdate>,
}

pub struct DictationService { /* private: Arc<Localisation>, current session, bridge state */ }

impl DictationService {
    pub fn new(localisation: Arc<Localisation>) -> Arc<DictationService>;

    /// Never asks for a permission; reads platform, OS version, locale support and permission state.
    pub async fn availability(&self) -> DictationAvailability;

    /// Starts listening for `field`. Asks for permissions on first use. Ends any running session
    /// first (its last update is `Ended { reason: Replaced }`).
    pub async fn start(&self, field: FieldRef) -> Result<DictationSession, DictationError>;

    /// Stops listening; remaining audio is finished into a last `Final`, then `Ended { Stopped }`.
    pub async fn stop(&self, session_id: DictationSessionId) -> Result<(), DictationError>;

    /// Stops at once; no further `Final`; `Ended { Cancelled }`.
    pub async fn cancel(&self, session_id: DictationSessionId) -> Result<(), DictationError>;
}

#[derive(Debug, thiserror::Error)]
pub enum DictationError {
    #[error("dictation unavailable")] Unavailable(DictationUnavailableReason), // DICT_UNAVAILABLE_*
    #[error("recognition assets could not be installed")] AssetsUnavailable, // DICT_ASSETS_UNAVAILABLE, retryable
    #[error("no audio input")] AudioInputFailed,                     // DICT_AUDIO_FAILED, retryable
    #[error("unknown session")] SessionUnknown,                      // DICT_SESSION_UNKNOWN
    #[error("field reference invalid")] FieldInvalid,                // DICT_FIELD_INVALID
    #[error("bridge failure: {0}")] Bridge(i32),                     // DICT_BRIDGE_FAILED, retryable
}
/// The one error contract of module errors (2C `CodedError`): `code()` as in each variant's
/// comment; `retryable()` true for `AssetsUnavailable`, `AudioInputFailed`, `Bridge`;
/// `repair()` is `Some(TryAgain)` for those three and `None` otherwise.
impl ordin_types::CodedError for DictationError {
    fn code(&self) -> &'static str;
    fn retryable(&self) -> bool;
    fn repair(&self) -> Option<ordin_types::RepairAction>;
}
```

### Swift bridge ABI (macOS 26+ only; `swift/Sources/OrdinDictation/Bridge.swift`)

```c
// Status codes returned by every call: 0 ok; 1 platform; 2 os_too_old; 3 language;
// 4 microphone_denied; 5 speech_denied; 6 assets_failed; 7 audio_failed; 8 no_session; 9 internal.
typedef void (*ordin_dict_callback)(void *ctx, int32_t kind, const char *utf8_text);
// kind: 0 preparing, 1 listening, 2 partial, 3 final, 4 ended_stopped, 5 ended_cancelled,
//       6 ended_silence, 7 ended_failed. utf8_text is NULL except for kinds 2 and 3;
//       it is valid only during the call and is copied by Rust.
int32_t ordin_dict_availability(const char *bcp47, int32_t *needs_assets_out);
int32_t ordin_dict_start(const char *bcp47, int32_t ask_permission, ordin_dict_callback cb, void *ctx);
int32_t ordin_dict_stop(void);
int32_t ordin_dict_cancel(void);
```

The Rust side declares these `extern "C"` behind `#[cfg(target_os = "macos")]`; `ctx` is a `Box<mpsc::Sender<…>>` leaked for the session and reclaimed on the `ended_*` callback. The callback is invoked on a Swift task's thread; the Rust callback only copies the text and calls `try_send` (channel capacity 256; on a full channel a `Partial` is dropped, a `Final` or ended kind is sent with `blocking_send` from a dedicated thread — see step C4).

### Service API (desktop only; never served by the Remote API)

| Command *(chosen here)* | Input | Result |
|---|---|---|
| `dictation.get_availability` | `{}` | `DictationAvailability` |
| `dictation.start` | `{"field": "<FieldRef>"}` + an IPC channel | `{"session_id": "<uuid>"}`; then `DictationUpdate` values on the channel |
| `dictation.stop` | `{"session_id"}` | `{}` |
| `dictation.cancel` | `{"session_id"}` | `{}` |

These four commands are the definitive ones; the Service API catalogue takes them as written (R1, R6) — there is no two-command form and no `DictatedText` result: text reaches the screens only as `Partial` and `Final` updates on the channel. The per-call update channel of `dictation.start` is a Tauri IPC channel the Command Bridge passes through for this one command (reconciliation row 190; the Command Bridge owns that mechanism).

### Who calls each public item (reconciliation §3; R6)

| Item | Called by |
|---|---|
| `DictationService::new` | desktop program (with `Arc<Localisation>`) |
| `availability`, `start`, `stop`, `cancel` | desktop program, for the four `dictation.*` commands, called by the screens' dictation button of a text field (`FieldRef`; updates `Partial`, `Final`, `Ended`) |
| `cancel` | desktop program, also on quit and when the computer goes to sleep, for the running session |

### Example

```json
// dictation.start
{"field":"conversation-message-box"}
// reply
{"session_id":"0192b3c4-1111-7222-8333-444455556666"}
// channel
{"kind":"listening","session_id":"0192b3c4-1111-7222-8333-444455556666"}
{"kind":"partial","session_id":"0192b3c4-…","text":"Please summarise the supplier"}
{"kind":"final","session_id":"0192b3c4-…","text":"Please summarise the supplier quotes from last week."}
{"kind":"ended","session_id":"0192b3c4-…","reason":"stopped"}
```

On Windows `dictation.get_availability` returns `{"state":"unavailable","reason":"windows","message":"Dictation isn't available on Windows."}`.

## Internal Logic

### A. Availability (no permission prompt)

1. Target not macOS → `Unavailable(Windows)` on Windows, `Unavailable(NotOffered)` on any other target (the server build).
2. macOS: `ProcessInfo.processInfo.isOperatingSystemAtLeast(OperatingSystemVersion(majorVersion: 26, …))` false → `MacosTooOld`.
3. Read `localisation.active().locale`; Swift checks `SpeechTranscriber.supportedLocales`, then `DictationTranscriber.supportedLocales`, matching by `Locale.Language` equality of the BCP 47 identifier; neither → `LanguageNotSupported`.
4. Permission state without prompting: `AVCaptureDevice.authorizationStatus(for: .audio)` `.denied`/`.restricted` → `MicrophoneDenied`; `SFSpeechRecognizer.authorizationStatus()` `.denied`/`.restricted` → `SpeechRecognitionDenied` (`.notDetermined` counts as available: asked at start).
5. Supported but assets not installed (`AssetInventory.status(forModules:)` not `.installed`) → `NeedsAssets`; else `Available`.
6. `message` for each unavailable reason is `localisation.message(<id>, &args)` — `args` empty, except `language` (`ArgValue::Text`) for the language message — with ids from *Names chosen here*; en-GB text: Windows and older macOS use the 11 wording verbatim; `dictation-unavailable-language` = "Dictation isn't available in { $language } yet." (`$language` = the locale's `locale-native-name`); `dictation-unavailable-microphone` = "Dictation needs the microphone. You can allow it in System Settings › Privacy & Security › Microphone."; `dictation-unavailable-speech` = "Dictation needs speech recognition. You can allow it in System Settings › Privacy & Security › Speech Recognition." (*chosen here*, Phase 4 wording review).
7. The screens hide the microphone button when the reason is `Windows`, `MacosTooOld` or `NotOffered` (ADR-078), and show it disabled with the message for the other three reasons (ADR-043: "the button says so").

### B. Start

1. Validate `field`: 1–128 chars, `[A-Za-z0-9._:-]`; else `FieldInvalid`.
2. If a session is running: `ordin_dict_stop()`, wait ≤ 2 s for its last `Final`, send `Ended { Replaced }` on its channel, drop it.
3. Re-run A1–A3; an unavailable result → `Unavailable(reason)`.
4. New `DictationSessionId` (UUIDv7); channel `mpsc::channel(256)`.
5. Call `ordin_dict_start(locale, ask_permission = 1, cb, ctx)`. Swift side, in order:
   a. `AVCaptureDevice.requestAccess(for: .audio)` if not determined; denied → status 4. `SFSpeechRecognizer.requestAuthorization` if not determined; denied → status 5. The system shows the plist usage descriptions (en-GB *chosen here*: microphone — "ordin listens only while you dictate, to turn your words into text on this Mac."; speech — "Your words are turned into text on this Mac and are never sent anywhere.").
   b. Choose the module: `SpeechTranscriber(locale:, transcriptionOptions: [], reportingOptions: [.volatileResults], attributeOptions: [])` when the locale is in its supported list, else `DictationTranscriber` with volatile results.
   c. If assets are not installed: callback `preparing`; `AssetInventory.assetInstallationRequest(supporting: [module])?.downloadAndInstall()`; failure → status 6. The download is the operating system's, from Apple; ordin downloads nothing (ADR-078).
   d. Create `SpeechAnalyzer(modules: [module])`, an `AVAudioEngine` input tap converting buffers to `SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith:)`, feed an `AsyncStream<AnalyzerInput>`; engine start failure or no input device → status 7.
   e. Callback `listening`; iterate `module.results`: a volatile result → `partial` with its `String(result.text.characters)`; a final result → `final`.
6. Status 0 → return `DictationSession`; other statuses map to the errors in the Error Table. Log `dictation.started` (`info`, fields `session_id`, `locale`, `module` = `speech_transcriber` | `dictation_transcriber`).

### C. During a session

1. **Silence**: no new result for 120 s → Swift finalises and calls `ended_silence` *(assumed: ends a forgotten open microphone)*.
2. **Time limit**: 10 minutes after `listening` → stop as in D with reason `TimeLimit` *(assumed: bounds an open microphone; ADR-043 "capture only while the person dictates")*.
3. **Locale change**: the `localisation.subscribe()` watcher seeing a new locale stops the session with reason `LocaleChanged`; the person starts again in the new locale.
4. **Back-pressure**: `Partial` updates are dropped when the channel is full (the next one replaces them anyway); `Final` and `Ended` are never dropped (sent from a dedicated relay thread with `blocking_send`).
5. Audio buffers exist only in the analyzer's input stream in memory; nothing is written to disk, no buffer is retained after the analyzer consumes it, and the engine tap is removed at session end. Text is held only until sent on the channel.
6. A mid-session analyzer error → `ended_failed`; Rust sends `Ended { Failed }`; the screens show `dictation-failed` ("Dictation stopped. You can try again." *chosen here*) with **Try again**. Logged `dictation.failed` (`warn`, fields `session_id`, `bridge_status`).

### D. Stop and cancel

1. Unknown or ended `session_id` → `SessionUnknown`.
2. `stop`: `ordin_dict_stop()` → Swift calls `analyzer.finalizeAndFinishThroughEndOfInput()`, emits remaining `final` results, then `ended_stopped`. Rust waits ≤ 2 s for it; past that it sends `Ended { Stopped }` itself and calls `ordin_dict_cancel()`.
3. `cancel`: `ordin_dict_cancel()` → Swift cancels the analyzer task and stops the engine at once, then `ended_cancelled`; no further `final`.
4. Log `dictation.ended` (`info`, fields `session_id`, `reason`, `duration_ms`, `final_count`).

### E. What the screens do with updates (contract for the screens module)

`Partial` replaces the provisional tail after the last `Final`; `Final` appends; nothing is sent until the person presses the field's own send or submit control. A dictated "yes", "approve" or "send" is plain text in the field and has no effect on any card (ADR-043). On an approval card there is no dictation control.

### F. Quit and sleep

On application quit or system sleep the program calls `cancel` on the running session.

## Data Model

N/A — the component stores nothing: no audio, no text, no model (ADR-043, ADR-078). The 2C *Locale preference* it reads is owned by Localisation.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Started on Windows | `Err(permanent)` | `DICT_UNAVAILABLE_WINDOWS` | `{"code":"DICT_UNAVAILABLE_WINDOWS","message":"Dictation isn't available on Windows.","action":null}` |
| Started on macOS before 26 | `Err(permanent)` | `DICT_UNAVAILABLE_MACOS_VERSION` | `{"code":"DICT_UNAVAILABLE_MACOS_VERSION","message":"Dictation isn't available on this version of macOS.","action":null}` |
| Started in a build with no dictation (server) | `Err(permanent)` | `DICT_NOT_OFFERED` | `{"code":"DICT_NOT_OFFERED","message":"Dictation isn't available here.","action":null}` |
| Locale supported by neither transcriber | `Err(permanent)` | `DICT_LANGUAGE_NOT_SUPPORTED` | `{"code":"DICT_LANGUAGE_NOT_SUPPORTED","message":"Dictation isn't available in Français yet.","action":null}` |
| Microphone permission denied or restricted (status 4) | `Err(permanent)` | `DICT_MICROPHONE_DENIED` | `{"code":"DICT_MICROPHONE_DENIED","message":"Dictation needs the microphone. You can allow it in System Settings › Privacy & Security › Microphone.","action":null}` |
| Speech recognition permission denied or restricted (status 5) | `Err(permanent)` | `DICT_SPEECH_DENIED` | `{"code":"DICT_SPEECH_DENIED","message":"Dictation needs speech recognition. You can allow it in System Settings › Privacy & Security › Speech Recognition.","action":null}` |
| System could not install recognition assets (status 6) | `Err(retryable)` | `DICT_ASSETS_UNAVAILABLE` | `{"code":"DICT_ASSETS_UNAVAILABLE","message":"Dictation couldn't get ready. Check your connection and try again.","action":"try_again"}` |
| No audio input device, or the audio engine failed to start (status 7) | `Err(retryable)` | `DICT_AUDIO_FAILED` | `{"code":"DICT_AUDIO_FAILED","message":"ordin couldn't hear your microphone.","action":"try_again"}` |
| `stop`/`cancel` with an unknown or ended session | `Err(permanent)` | `DICT_SESSION_UNKNOWN` | `{"code":"DICT_SESSION_UNKNOWN","message":"Dictation has already stopped.","action":null}` |
| `field` outside 1–128 chars or the allowed characters | `Err(permanent)`; also refused as `SVC_INPUT_INVALID` by schema when the type allows | `DICT_FIELD_INVALID` | `{"code":"DICT_FIELD_INVALID","message":"Dictation couldn't start here.","action":null}` |
| Bridge returned status 9, or an analyzer error mid-session | `Err(retryable)` / `Ended { Failed }` | `DICT_BRIDGE_FAILED` | `{"code":"DICT_BRIDGE_FAILED","message":"Dictation stopped. You can try again.","action":"try_again"}` |

Messages other than the two 11 sentences are *chosen here* and pass the Phase 4 wording review.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Dictation Service

  Scenario: Happy path — dictate into the message box
    Given macOS 26, locale "en-GB", microphone and speech recognition allowed, assets installed
    When the screens send dictation.start for field "conversation-message-box"
    Then a session id is returned
    And the channel receives "listening", then "partial" updates, then a "final" with the recognised text
    When the screens send dictation.stop
    Then the channel ends with "ended" reason "stopped"
    And nothing is sent to Ordin until the person presses Send

  Scenario: Edge — first use asks for permissions
    Given macOS 26 and microphone permission not yet determined
    When dictation.start is sent
    Then the operating system asks for the microphone with the usage description
    And after the person allows it the session starts listening

  Scenario: Edge — assets installed on first use
    Given the locale is supported but its recognition assets are not installed
    When dictation.start is sent
    Then the channel receives "preparing" before "listening"
    And ordin itself downloads nothing

  Scenario: Error — Windows
    Given the desktop runs on Windows
    When the screens send dictation.get_availability
    Then the result is unavailable with reason "windows" and message "Dictation isn't available on Windows."
    And the microphone button is not shown

  Scenario: Error — older macOS
    Given macOS 15
    When dictation.start is sent
    Then the reply is an error with code DICT_UNAVAILABLE_MACOS_VERSION

  Scenario: Error — language not supported
    Given the active locale is one neither SpeechTranscriber nor DictationTranscriber supports
    When dictation.get_availability is sent
    Then the result is unavailable with reason "language_not_supported"
    And the microphone button is shown disabled with that message

  Scenario: Error — microphone denied
    Given the microphone permission is denied in System Settings
    When dictation.start is sent
    Then the reply is an error with code DICT_MICROPHONE_DENIED

  Scenario: Error — no input device
    Given no microphone is connected
    When dictation.start is sent
    Then the reply is an error with code DICT_AUDIO_FAILED and action "try_again"

  Scenario: Edge — starting in another field replaces the session
    Given a session is listening for field "form-field-1"
    When dictation.start is sent for field "form-field-2"
    Then the first session's channel ends with reason "replaced"
    And the second session starts listening

  Scenario: Edge — silence ends the session
    Given a session is listening
    When no speech is recognised for 120 seconds
    Then the channel ends with reason "silence"

  Scenario: Edge — locale change stops dictation
    Given a session is listening in "en-GB"
    When the person's locale changes to "fr-FR"
    Then the channel ends with reason "locale_changed"

  Scenario: Edge — spoken "approve" is only text
    Given an approval card is waiting
    When the person dictates "approve" into the message box
    Then the word appears in the message box and the approval stays pending

  Scenario: Error — stop for an unknown session
    When dictation.stop is sent with a session id that has ended
    Then the reply is an error with code DICT_SESSION_UNKNOWN

  Scenario: Edge — no audio kept
    Given a session has ended
    Then no audio file exists under the application data root or the temporary folder
    And no log line contains the dictated text

  Scenario: Edge — the desktop program cancels a running session on sleep
    Given a dictation session is listening
    When the computer goes to sleep
    Then the desktop program calls cancel for that session
    And the last update is Ended with reason cancelled and no Final follows
```

## Performance, Security, Observability

- **Performance targets** (macOS 26 on Apple silicon, assets installed):
  - `dictation.get_availability`: p95 ≤ 50 ms.
  - `dictation.start` to `listening`: p95 ≤ 400 ms when permissions are already granted.
  - First `partial` within 1 s of the start of speech (p95); a `final` within 700 ms of the end of a phrase (p95) *(assumed: matches the 1 s first-progress target of 07)*.
  - Memory added by a session: ≤ 150 MiB (the system's model is loaded by the OS); CPU returns to idle within 2 s of `ended`.
- **Security:**
  - Entry points: four desktop-only commands through the Command Bridge's Tauri capability; never on the Remote API. Inputs validated (field reference pattern, UUIDv7 session id; unknown fields refused).
  - Audio never leaves the computer and is never written (no file, no cache); the engine tap is removed at session end; recognition is on-device (ADR-043).
  - Dictated text is never logged and never journalled by this component; it reaches the journal only as part of a message the person sends.
  - The microphone is open only between `listening` and `ended`; the 120 s silence and 10-minute limits close a forgotten session. macOS shows its own microphone indicator.
  - The text never triggers an action; the component has no send or approve path (decision 10, ADR-043).
- **Observability** (`tracing` JSON lines; desktop only, no metrics or spans, SA-XC-04/05):

| Event | Level | Fields |
|---|---|---|
| `dictation.started` | info | `session_id`, `locale`, `module` |
| `dictation.ended` | info | `session_id`, `reason`, `duration_ms`, `final_count` |
| `dictation.unavailable` | info | `reason`, `locale` |
| `dictation.permission_denied` | info | `permission` (`microphone` \| `speech_recognition`) |
| `dictation.assets_failed` | warn | `locale` |
| `dictation.failed` | warn | `session_id`, `bridge_status` |

Every line carries `component = "dictation"`. Never logged: recognised text, audio levels, field contents.

## Gaps

None.
