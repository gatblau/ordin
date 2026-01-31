//! The OBS Recorder Adapter (ObsRecorderAdapter.md): finds whether OBS is installed, talks to its
//! WebSocket server (obs-websocket protocol version 5) on this computer, prepares the sound-only
//! scene **Ordin meeting**, and starts and stops a recording for the Minutes Service.
//!
//! It never installs OBS and never changes the person's own scenes or profile: the recording
//! settings live in Ordin's own profile, selected only while recording. The WebSocket password is
//! read from Identity and Credentials per connection and never logged.
//!
//! Conditional assertion relied on: C-111 (the profile parameters and input kinds of Internal
//! Logic C3–C4), settled against a real OBS in plan 07 Phase 9.
#![deny(unsafe_code)]

mod detect;
mod error;
mod protocol;
mod recorder;
mod types;

#[cfg(test)]
mod tests;

/// A fake OBS for the tests of this crate and of the Minutes Service (plan 07 Phase 5); not in any
/// program's build.
#[cfg(any(test, feature = "fake"))]
#[doc(hidden)]
pub mod fake;

pub use error::{ObsError, ALL_CODES};
pub use recorder::ObsRecorder;
// SPEC-GAP-1: ObsRecorderAdapter.md *Names chosen here* lists `ObsStatus`, which the Public Interface
// never defines; it is not invented here (plan 07 FU-008).
pub use types::{ObsState, RecordingFinished, RecordingStarted};
