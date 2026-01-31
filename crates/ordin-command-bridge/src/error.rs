//! The Bridge's own failures and the codes it owns (CommandBridge.md *Public Interface*, *Error
//! Table*; ErrorHandling.md *Internal Logic* 11). Command refusals keep the Service API's `SVC_` codes
//! and a server's failures keep the Server Connector's `CONN_` codes; neither is owned here.

use ordin_types::CodedError;
use serde::Serialize;

/// Every code the registry gives this crate: its `BRG_` rows. `BRG_JOURNAL_LAGGED` and
/// `BRG_WINDOW_LAGGING` are logged rows with no error value; `BRG_CLOSING` travels inside a command
/// reply.
pub const ALL_CODES: &[&str] = &[
    "BRG_BACKEND_UNKNOWN",
    "BRG_CLOSING",
    "BRG_JOURNAL_LAGGED",
    "BRG_STREAM_LAGGING",
    "BRG_STREAM_NO_WINDOW",
    "BRG_WINDOW_LAGGING",
];

pub(crate) const CODE_BACKEND_UNKNOWN: &str = "BRG_BACKEND_UNKNOWN";
pub(crate) const CODE_CLOSING: &str = "BRG_CLOSING";
pub(crate) const CODE_JOURNAL_LAGGED: &str = "BRG_JOURNAL_LAGGED";
pub(crate) const CODE_WINDOW_LAGGING: &str = "BRG_WINDOW_LAGGING";

/// The failure of a transport command outside the catalogue (`service_events`, `service_streams`).
/// Serialised for the WebView as `{"code":"BRG_…"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
#[error("{code}")]
pub struct BridgeError {
    pub code: &'static str,
}

impl BridgeError {
    pub(crate) fn backend_unknown() -> BridgeError {
        BridgeError {
            code: CODE_BACKEND_UNKNOWN,
        }
    }
}

/// Every `BridgeError` the Error Table gives is permanent: the window must call `service.hello` /
/// `servers.list` again before it opens the stream.
impl CodedError for BridgeError {
    fn code(&self) -> &'static str {
        self.code
    }

    fn retryable(&self) -> bool {
        false
    }
}

/// Why [`crate::ScreenStreams::send`] could not queue a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StreamSendError {
    #[error("BRG_STREAM_NO_WINDOW")]
    NoWindow,
    #[error("BRG_STREAM_LAGGING")]
    Lagging,
}

/// Both rows are permanent for the message in hand: the program drops it (NoWindow) or slows its
/// producer (Lagging); sending the same message again is the producer's choice, not a retry.
impl CodedError for StreamSendError {
    fn code(&self) -> &'static str {
        match self {
            StreamSendError::NoWindow => "BRG_STREAM_NO_WINDOW",
            StreamSendError::Lagging => "BRG_STREAM_LAGGING",
        }
    }

    fn retryable(&self) -> bool {
        false
    }
}
