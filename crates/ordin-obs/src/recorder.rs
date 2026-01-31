//! `ObsRecorder` (ObsRecorderAdapter.md *Public Interface*, Internal Logic B–D).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};

use ordin_identity::IdentityService;
use ordin_localisation::Localisation;
use ordin_types::time::Timestamp;
use serde_json::{json, Value};

use crate::detect::{Host, Platform};
use crate::error::ObsError;
use crate::protocol::{self, Limits, Session};
use crate::types::{ObsState, RecordingFinished, RecordingStarted};

/// The default WebSocket port of OBS.
const DEFAULT_PORT: u16 = 4455;
/// The lowest port the person may choose.
const LOWEST_PORT: u16 = 1024;
/// Ordin's own profile and scene in OBS (fixed data, not translated).
const ORDIN_MEETING: &str = "Ordin meeting";
const INPUT_YOU: &str = "You";
const INPUT_OTHERS: &str = "Others";
/// The oldest OBS spoken to, and the first that records the computer's sound on macOS.
const OLDEST_OBS: (u64, u64, u64) = (28, 0, 0);
const MAC_SYSTEM_SOUND_OBS: (u64, u64, u64) = (30, 0, 0);
/// How often `stop` looks for the finished file.
const OUTPUT_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// C3: the recording settings of Ordin's profile (conditional on C-111).
const PROFILE_PARAMETERS: [(&str, &str, &str); 6] = [
    ("Output", "Mode", "Advanced"),
    ("AdvOut", "RecType", "Standard"),
    ("AdvOut", "RecFormat2", "mkv"),
    // Tracks 1 and 2, as a bit mask.
    ("AdvOut", "RecTracks", "3"),
    ("AdvOut", "Track1Name", INPUT_YOU),
    ("AdvOut", "Track2Name", INPUT_OTHERS),
];

/// The person's profile and scene before Ordin's were selected (C2), in memory only.
#[derive(Debug, Clone)]
struct Remembered {
    profile: String,
    scene: String,
}

/// An identified connection and what `GetVersion` said (B4).
struct Connected {
    session: Session,
    obs_version: String,
    others_heard: bool,
}

pub struct ObsRecorder {
    identity: Arc<IdentityService>,
    // The adapter's sentences are rendered by its callers from the error codes; kept for the
    // interface of ObsRecorderAdapter.md.
    #[allow(dead_code)]
    localisation: Arc<Localisation>,
    port: AtomicU16,
    remembered: Mutex<Option<Remembered>>,
    /// One of `start` and `stop` at a time, so two cannot interleave their requests.
    operation: tokio::sync::Mutex<()>,
    host: Host,
    limits: Limits,
}

impl ObsRecorder {
    pub fn new(identity: Arc<IdentityService>, localisation: Arc<Localisation>) -> Arc<Self> {
        Self::with_host(identity, localisation, Host::current(), Limits::default())
    }

    pub(crate) fn with_host(
        identity: Arc<IdentityService>,
        localisation: Arc<Localisation>,
        host: Host,
        limits: Limits,
    ) -> Arc<Self> {
        Arc::new(ObsRecorder {
            identity,
            localisation,
            port: AtomicU16::new(DEFAULT_PORT),
            remembered: Mutex::new(None),
            operation: tokio::sync::Mutex::new(()),
            host,
            limits,
        })
    }

    /// An adapter that finds OBS installed on Windows (so both tracks are recorded) and talks to
    /// the fake of [`crate::fake`]; for the Minutes Service's tests only.
    #[cfg(feature = "fake")]
    #[doc(hidden)]
    pub fn for_fake(identity: Arc<IdentityService>, localisation: Arc<Localisation>) -> Arc<Self> {
        let host = Host {
            platform: Platform::Windows,
            detect: Arc::new(|| Some(PathBuf::from("obs64.exe"))),
            launch: Arc::new(|_: &std::path::Path| Ok(())),
        };
        let limits = Limits {
            output_wait: std::time::Duration::from_millis(500),
            output_settled: std::time::Duration::from_millis(100),
            ..Limits::default()
        };
        Self::with_host(identity, localisation, host, limits)
    }

    /// Port 1024–65535; default 4455. Takes effect at the next call.
    pub fn set_port(&self, port: u16) -> Result<(), ObsError> {
        if port < LOWEST_PORT {
            return Err(ObsError::PortInvalid);
        }
        self.port.store(port, Ordering::SeqCst);
        Ok(())
    }

    /// Detects the installation and, when installed, connects and identifies; never starts OBS.
    pub async fn state(&self) -> ObsState {
        let state = self.read_state().await;
        tracing::debug!(component = "obs", state = state_str(&state), "obs.state");
        state
    }

    async fn read_state(&self) -> ObsState {
        if (self.host.detect)().is_none() {
            return ObsState::NotInstalled;
        }
        let mut connected = match self.connect().await {
            Ok(connected) => connected,
            Err((error, obs_version)) => return state_of(error, obs_version),
        };
        let status = connected.session.request("GetRecordStatus", None).await;
        connected.session.close().await;
        match status {
            Ok(status) if status.get("outputActive").and_then(Value::as_bool) == Some(true) => {
                let duration = millis(&status, "outputDuration");
                let started = Timestamp::now()
                    .unix_millis()
                    .saturating_sub(i64::try_from(duration).unwrap_or(i64::MAX));
                ObsState::Recording {
                    started_at: Timestamp::from_unix_millis(started)
                        .unwrap_or_else(|_| Timestamp::now()),
                    others_heard: connected.others_heard,
                }
            }
            Ok(_) => ObsState::Ready {
                obs_version: connected.obs_version,
                others_heard: connected.others_heard,
            },
            Err(error) => state_of(error, connected.obs_version),
        }
    }

    /// Starts OBS (macOS: `open -a <bundle>`; Windows: the detected `obs64.exe`, working directory
    /// its folder).
    pub async fn open_obs(&self) -> Result<(), ObsError> {
        let program = (self.host.detect)().ok_or(ObsError::NotInstalled)?;
        // SPEC-GAP-2: ObsRecorderAdapter.md names no code for a program that is found but does not
        // start; `NotReachable` asks the person to open OBS themselves (plan 07 FU-008).
        (self.host.launch)(&program).map_err(|_| ObsError::NotReachable)
    }

    /// Ensures the profile and scene, reads the recording folder, then sends StartRecord.
    pub async fn start(&self) -> Result<RecordingStarted, ObsError> {
        let _one = self.operation.lock().await;
        let mut connected = self.connect().await.map_err(|(error, _)| error)?;
        let result = self.start_on(&mut connected).await;
        connected.session.close().await;
        result
    }

    async fn start_on(&self, connected: &mut Connected) -> Result<RecordingStarted, ObsError> {
        let session = &mut connected.session;
        // C1.
        let status = session.request("GetRecordStatus", None).await?;
        if status.get("outputActive").and_then(Value::as_bool) == Some(true) {
            return Err(ObsError::AlreadyRecording);
        }
        // C2.
        let profiles = session.request("GetProfileList", None).await?;
        let scene = session.request("GetCurrentProgramScene", None).await?;
        let remembered = Remembered {
            profile: text(&profiles, "currentProfileName")?,
            // `sceneName` from obs-websocket 5.3 (OBS 30); `currentProgramSceneName` before it.
            scene: text(&scene, "sceneName")
                .or_else(|_| text(&scene, "currentProgramSceneName"))?,
        };
        // C3–C4: any failure restores the person's profile and scene.
        if prepare(
            session,
            &profiles,
            &remembered.profile,
            self.host.platform,
            connected.others_heard,
        )
        .await
        .is_err()
        {
            restore(session, &remembered).await;
            return Err(ObsError::SceneFailed);
        }
        // C5, C6, then D1.
        let started = async {
            session
                .request(
                    "SetCurrentProgramScene",
                    Some(json!({ "sceneName": ORDIN_MEETING })),
                )
                .await?;
            // C6: the folder of the Ordin meeting profile, selected at C3.
            let directory = session.request("GetRecordDirectory", None).await?;
            let record_folder = text(&directory, "recordDirectory")
                .ok()
                .filter(|folder| !folder.is_empty())
                .map(PathBuf::from)
                .ok_or_else(|| logged(ObsError::Request(0), "GetRecordDirectory"))?;
            session.request("StartRecord", None).await?;
            Ok::<PathBuf, ObsError>(record_folder)
        }
        .await;
        let record_folder = match started {
            Ok(record_folder) => record_folder,
            Err(error) => {
                restore(session, &remembered).await;
                return Err(error);
            }
        };
        *self.remembered.lock().unwrap_or_else(|e| e.into_inner()) = Some(remembered);
        tracing::info!(
            component = "obs",
            obs_version = connected.obs_version.as_str(),
            others_heard = connected.others_heard,
            "obs.recording_started"
        );
        Ok(RecordingStarted {
            started_at: Timestamp::now(),
            others_heard: connected.others_heard,
            record_folder,
        })
    }

    /// Sends StopRecord, restores the person's profile and scene, returns the file.
    pub async fn stop(&self) -> Result<RecordingFinished, ObsError> {
        let _one = self.operation.lock().await;
        let mut connected = self.connect().await.map_err(|(error, _)| error)?;
        let result = self.stop_on(&mut connected.session).await;
        connected.session.close().await;
        let finished = result?;
        tracing::info!(
            component = "obs",
            duration_ms = finished.duration_ms,
            "obs.recording_stopped"
        );
        Ok(finished)
    }

    async fn stop_on(&self, session: &mut Session) -> Result<RecordingFinished, ObsError> {
        // D2.
        let status = session.request("GetRecordStatus", None).await?;
        if status.get("outputActive").and_then(Value::as_bool) != Some(true) {
            return Err(ObsError::NotRecording);
        }
        let duration_ms = millis(&status, "outputDuration");
        let stopped = session.request("StopRecord", None).await?;
        let remembered = self
            .remembered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(remembered) = remembered {
            restore(session, &remembered).await;
        }
        let output_path = stopped
            .get("outputPath")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .ok_or(ObsError::OutputMissing)?;
        // OBS-STOP-1: the file is finished when it is a file, not empty, and its size has stayed
        // the same for `output_settled`; OBS answers StopRecord before it has closed the file.
        // spec: ordin-desktop/components/ObsRecorderAdapter#OBS-STOP-1
        let deadline = tokio::time::Instant::now() + self.limits.output_wait;
        let mut last: Option<(u64, tokio::time::Instant)> = None;
        loop {
            let now = tokio::time::Instant::now();
            let size = std::fs::metadata(&output_path)
                .ok()
                .filter(std::fs::Metadata::is_file)
                .map(|m| m.len())
                .filter(|len| *len > 0);
            match (size, last) {
                (Some(size), Some((seen, since))) if size == seen => {
                    if now.duration_since(since) >= self.limits.output_settled {
                        return Ok(RecordingFinished {
                            output_path,
                            duration_ms,
                        });
                    }
                }
                (Some(size), _) => last = Some((size, now)),
                (None, _) => last = None,
            }
            if now >= deadline {
                return Err(ObsError::OutputMissing);
            }
            tokio::time::sleep(OUTPUT_POLL).await;
        }
    }

    /// B1–B4. The error comes with the OBS version when `GetVersion` was answered.
    async fn connect(&self) -> Result<Connected, (ObsError, String)> {
        let port = self.port.load(Ordering::SeqCst);
        let (socket, hello) = protocol::connect(port, self.limits)
            .await
            .map_err(|error| (logged(error, "Hello"), String::new()))?;
        let password = match &hello.authentication {
            None => None,
            // SPEC-GAP-3: ObsRecorderAdapter.md B2 names no code for a credential store that
            // cannot be read; it is reported as `PasswordNeeded`, whose remedy (saving the
            // password again) surfaces the store's own error (plan 07 FU-008).
            Some(_) => match self.identity.obs_password().await {
                Ok(Some(password)) => Some(password),
                Ok(None) | Err(_) => {
                    return Err((logged(ObsError::PasswordNeeded, "Identify"), String::new()))
                }
            },
        };
        let mut session = Session::identify(socket, &hello, password, self.limits)
            .await
            .map_err(|error| (logged(error, "Identify"), String::new()))?;
        let version = match session.request("GetVersion", None).await {
            Ok(version) => version,
            Err(error) => {
                session.close().await;
                return Err((error, String::new()));
            }
        };
        let obs_version = version
            .get("obsVersion")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let parts = protocol::version_parts(&obs_version);
        if parts < OLDEST_OBS {
            session.close().await;
            return Err((
                logged(ObsError::VersionUnsupported, "GetVersion"),
                obs_version,
            ));
        }
        let others_heard = match self.host.platform {
            Platform::Windows => true,
            Platform::MacOs => parts >= MAC_SYSTEM_SOUND_OBS,
            Platform::Other => false,
        };
        Ok(Connected {
            session,
            obs_version,
            others_heard,
        })
    }
}

/// C3–C4. A present scene is left as it is: the person may have chosen other devices in it.
async fn prepare(
    session: &mut Session,
    profiles: &Value,
    remembered_profile: &str,
    platform: Platform,
    others_heard: bool,
) -> Result<(), ObsError> {
    let has_profile = profiles
        .get("profiles")
        .and_then(Value::as_array)
        .is_some_and(|list| list.iter().any(|p| p.as_str() == Some(ORDIN_MEETING)));
    if !has_profile {
        session
            .request(
                "CreateProfile",
                Some(json!({ "profileName": ORDIN_MEETING })),
            )
            .await?;
    }
    session
        .request(
            "SetCurrentProfile",
            Some(json!({ "profileName": ORDIN_MEETING })),
        )
        .await?;
    for (category, name, value) in PROFILE_PARAMETERS {
        session
            .request(
                "SetProfileParameter",
                Some(json!({
                    "parameterCategory": category,
                    "parameterName": name,
                    "parameterValue": value,
                })),
            )
            .await?;
    }
    reload_profile(session, profiles, remembered_profile).await?;

    let scenes = session.request("GetSceneList", None).await?;
    let has_scene = scenes
        .get("scenes")
        .and_then(Value::as_array)
        .is_some_and(|list| {
            list.iter()
                .any(|s| s.get("sceneName").and_then(Value::as_str) == Some(ORDIN_MEETING))
        });
    if has_scene {
        return Ok(());
    }
    session
        .request("CreateScene", Some(json!({ "sceneName": ORDIN_MEETING })))
        .await?;
    let (microphone, system_sound) = input_kinds(platform);
    let mut inputs = vec![(INPUT_YOU, microphone, 1)];
    if others_heard {
        inputs.push((INPUT_OTHERS, system_sound, 2));
    }
    for (name, kind, track) in inputs {
        session
            .request(
                "CreateInput",
                Some(json!({
                    "sceneName": ORDIN_MEETING,
                    "inputName": name,
                    "inputKind": kind,
                    "inputSettings": {},
                    "sceneItemEnabled": true,
                })),
            )
            .await?;
        session
            .request(
                "SetInputAudioTracks",
                Some(json!({ "inputName": name, "inputAudioTracks": tracks_only(track) })),
            )
            .await?;
    }
    Ok(())
}

/// The microphone and the computer's sound inputs, each on its default device (C4).
fn input_kinds(platform: Platform) -> (&'static str, &'static str) {
    if platform == Platform::Windows {
        ("wasapi_input_capture", "wasapi_output_capture")
    } else {
        ("coreaudio_input_capture", "sck_audio_capture")
    }
}

/// OBS's six audio tracks with only `track` on.
fn tracks_only(track: u8) -> Value {
    let mut tracks = serde_json::Map::new();
    for number in 1..=6u8 {
        tracks.insert(number.to_string(), Value::Bool(number == track));
    }
    Value::Object(tracks)
}

/// Back to the person's profile and scene; failures are logged, not returned.
/// OBS-PRO-1: OBS builds its recording output from a profile's settings when the profile is
/// selected, and `SetProfileParameter` only writes them, so **Ordin meeting** is selected again
/// through a partner profile. OBS-PRO-2: with no other profile, the recording goes ahead as it is.
// spec: ordin-desktop/components/ObsRecorderAdapter#OBS-PRO-1
// spec: ordin-desktop/components/ObsRecorderAdapter#OBS-PRO-2
async fn reload_profile(
    session: &mut Session,
    profiles: &Value,
    remembered_profile: &str,
) -> Result<(), ObsError> {
    let partner = partner_profile(profiles, remembered_profile);
    let Some(partner) = partner else {
        tracing::warn!(component = "obs", "obs.profile_not_reloaded");
        return Ok(());
    };
    for name in [partner.as_str(), ORDIN_MEETING] {
        session
            .request("SetCurrentProfile", Some(json!({ "profileName": name })))
            .await?;
    }
    Ok(())
}

/// The remembered profile when it is not **Ordin meeting**; else the first other listed one.
// spec: ordin-desktop/components/ObsRecorderAdapter#OBS-PRO-1
fn partner_profile(profiles: &Value, remembered_profile: &str) -> Option<String> {
    if remembered_profile != ORDIN_MEETING {
        return Some(remembered_profile.to_owned());
    }
    profiles
        .get("profiles")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .find(|name| *name != ORDIN_MEETING)
        .map(str::to_owned)
}

async fn restore(session: &mut Session, remembered: &Remembered) {
    let profile = session
        .request(
            "SetCurrentProfile",
            Some(json!({ "profileName": remembered.profile })),
        )
        .await;
    if profile.is_err() {
        tracing::warn!(component = "obs", what = "profile", "obs.restore_failed");
    }
    let scene = session
        .request(
            "SetCurrentProgramScene",
            Some(json!({ "sceneName": remembered.scene })),
        )
        .await;
    if scene.is_err() {
        tracing::warn!(component = "obs", what = "scene", "obs.restore_failed");
    }
}

/// A text field of an answer; missing → `Request(0)`, as OBS answered with something unexpected.
fn text(answer: &Value, field: &str) -> Result<String, ObsError> {
    answer
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(ObsError::Request(0))
}

fn millis(answer: &Value, field: &str) -> u64 {
    answer.get(field).and_then(Value::as_u64).unwrap_or(0)
}

fn logged(error: ObsError, request_type: &'static str) -> ObsError {
    tracing::warn!(
        component = "obs",
        code = ordin_types::CodedError::code(&error),
        request_type,
        "obs.failed"
    );
    error
}

/// The state a failed connection or request leaves.
fn state_of(error: ObsError, obs_version: String) -> ObsState {
    match error {
        ObsError::NotInstalled => ObsState::NotInstalled,
        ObsError::PasswordNeeded => ObsState::PasswordNeeded,
        ObsError::PasswordRefused => ObsState::PasswordRefused,
        ObsError::VersionUnsupported => ObsState::VersionUnsupported { obs_version },
        _ => ObsState::NotReachable,
    }
}

fn state_str(state: &ObsState) -> &'static str {
    match state {
        ObsState::NotInstalled => "not_installed",
        ObsState::NotReachable => "not_reachable",
        ObsState::PasswordNeeded => "password_needed",
        ObsState::PasswordRefused => "password_refused",
        ObsState::VersionUnsupported { .. } => "version_unsupported",
        ObsState::Ready { .. } => "ready",
        ObsState::Recording { .. } => "recording",
    }
}
