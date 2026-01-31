//! The fixed constants of HookRelay.md *Fixed constants*, and the timing set the tests scale down.

use std::time::Duration;

use ordin_types::HookCallId;

/// Hook protocol version (2C, 08:27). With protocol 1 current, only 1 is accepted.
pub(crate) const PROTOCOL: u16 = 1;
/// Request line limit: 1 MiB (2C wire §4); the newline is the one byte more.
pub(crate) const MAX_LINE_BYTES: usize = 1_048_576;
/// Replay set per session: the last 10,000 `call_id` values.
pub(crate) const REPLAY_CAPACITY: usize = 10_000;
/// The tool-use map is bounded like the replay set (Performance: memory per session under 1 MiB).
pub(crate) const TOOL_USE_CAPACITY: usize = 10_000;
/// Concurrent connections; the 65th waits in the accept backlog.
pub(crate) const MAX_CONNECTIONS: usize = 64;
/// Capacity of each session's broadcast sender (F.1).
pub(crate) const EVENT_CAPACITY: usize = 64;
/// `Deny.reason` is 1–500 characters.
pub(crate) const REASON_MAX_CHARS: usize = 500;
/// Rows older than 30 days are deleted at start (Data Model).
pub(crate) const LATE_MAX_AGE_MS: i64 = 30 * 24 * 3_600 * 1_000;
/// `ToolOrigin` label cut length of an exported span field (Tracing `MAX_LABEL_CHARS`).
pub(crate) const SPAN_LABEL_MAX_CHARS: usize = 128;

/// The nil UUID, used wherever the call id cannot be read.
pub(crate) fn nil_call_id() -> HookCallId {
    HookCallId(uuid::Uuid::nil())
}

/// The four durations of the relay. [`Timings::SPEC`] holds the spec's values; the crate's own tests
/// build shorter sets so that a 50 s hold does not take 50 s.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timings {
    /// Connection open, no complete line: 5 s.
    pub read: Duration,
    /// Decision budget for allow or deny: 1.5 s, inside the hook's 2 s.
    pub decision: Duration,
    /// Hook deadline in `Holding.deadline`: request receipt + 50 s.
    pub hold: Duration,
    /// Shutdown waits this long for open connections: 2 s.
    pub shutdown_grace: Duration,
}

impl Timings {
    pub(crate) const SPEC: Timings = Timings {
        read: Duration::from_secs(5),
        decision: Duration::from_millis(1_500),
        hold: Duration::from_secs(50),
        shutdown_grace: Duration::from_secs(2),
    };
}
