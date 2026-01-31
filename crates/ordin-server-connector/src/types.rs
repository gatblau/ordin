//! The public value types of the crate (ServerConnector.md *Public Interface*) and its timing constants
//! (*Constants*, Internal Logic).

use std::sync::Arc;
use std::time::Duration;

use ordin_service_api::{EventEnvelope, ServiceApiVersion};
use ordin_types::BackendId;

/// Upper bound of cached read replies per server when the program has no other figure: 32 MiB.
pub const DEFAULT_READ_CACHE_BYTES: usize = 32 * 1024 * 1024;

/// What the desktop program passes in at construction.
#[derive(Debug, Clone)]
pub struct ConnectorConfig {
    /// This computer's name, sent when pairing (1–63 characters).
    pub computer_name: String,
    /// From the organisation profile (`server.client_certificate_required`, `…_name`).
    pub client_certificate: Option<String>,
    /// Upper bound of cached read replies per server; default 32 MiB ([`DEFAULT_READ_CACHE_BYTES`]).
    pub read_cache_bytes: usize,
}

/// What the Command Bridge receives from a server's stream, in order.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    Event {
        backend: BackendId,
        envelope: Arc<EventEnvelope>,
    },
    Reset {
        backend: BackendId,
    },
}

/// The Command Bridge's channel.
pub type ServerEventSender = tokio::sync::mpsc::Sender<ServerEvent>;

/// A test seam, in the manner of the other adapters' stand-ins: the connector reaches a loopback
/// stand-in over plain HTTP, every wait of the spec is multiplied by `time_scale`, and the desktop's
/// own Service API version can differ from `ServiceApiVersion::CURRENT`. A program never uses it.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct StandIn {
    pub plain_http: bool,
    pub time_scale: f64,
    pub desktop_version: ServiceApiVersion,
}

/// The waits and limits of the spec, each multiplied by the stand-in's `time_scale` (1.0 in a program).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    scale: f64,
}

impl Timing {
    pub(crate) fn new(scale: f64) -> Self {
        Timing { scale }
    }

    pub(crate) fn scaled(&self, d: Duration) -> Duration {
        d.mul_f64(self.scale)
    }

    /// Not connected after 45 s without an event or heartbeat.
    pub(crate) fn silence(&self) -> Duration {
        self.scaled(Duration::from_secs(45))
    }

    /// Connect timeout of the client.
    pub(crate) fn connect(&self) -> Duration {
        self.scaled(Duration::from_secs(5))
    }

    /// HTTP/2 keep-alive interval of the client.
    pub(crate) fn keep_alive(&self) -> Duration {
        self.scaled(Duration::from_secs(20))
    }

    /// Timeout of `POST /v1/pairing`.
    pub(crate) fn pairing(&self) -> Duration {
        self.scaled(Duration::from_secs(15))
    }

    /// Timeout of `POST /v1/pairing/end`.
    pub(crate) fn end(&self) -> Duration {
        self.scaled(Duration::from_secs(10))
    }

    /// How often `last_sequence_received` is persisted while it changed.
    pub(crate) fn persist(&self) -> Duration {
        self.scaled(Duration::from_secs(5))
    }

    /// The wait after `DESK_ADDRESS_LOCKED` on the stream.
    pub(crate) fn locked(&self) -> Duration {
        self.scaled(Duration::from_secs(600))
    }

    /// A stream silent for longer than this is dropped when the computer wakes.
    pub(crate) fn stale(&self) -> Duration {
        self.scaled(Duration::from_secs(20))
    }

    /// Wait before the first and second retry of a command.
    pub(crate) fn retry(&self, retry: u32) -> Duration {
        self.scaled(Duration::from_millis(if retry == 0 { 200 } else { 400 }))
    }
}

/// Step 6.6: the wait before reconnection `attempt` (0 for the first): 1, 2, 4, 8, 16, 32, 60, 60 …
/// seconds, multiplied by `jitter`, which the caller draws in [0.8, 1.2].
pub(crate) fn backoff_base(attempt: u32) -> Duration {
    Duration::from_secs((1u64 << attempt.min(6)).min(60))
}

/// A random factor in [0.8, 1.2] from the operating system's random source via a v4 UUID.
pub(crate) fn jitter_factor() -> f64 {
    let bits = uuid::Uuid::new_v4().as_u128();
    let unit = (bits & 0xFFFF_FFFF) as f64 / f64::from(u32::MAX);
    0.8 + 0.4 * unit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_follows_the_schedule() {
        let seconds: Vec<u64> = (0..9).map(|a| backoff_base(a).as_secs()).collect();
        assert_eq!(seconds, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
        assert_eq!(backoff_base(u32::MAX).as_secs(), 60);
    }

    #[test]
    fn jitter_stays_within_twenty_percent() {
        for _ in 0..1000 {
            let f = jitter_factor();
            assert!((0.8..=1.2).contains(&f), "{f}");
        }
    }

    #[test]
    fn timing_scales_every_wait() {
        let t = Timing::new(0.5);
        assert_eq!(t.silence(), Duration::from_millis(22_500));
        assert_eq!(t.retry(0), Duration::from_millis(100));
        assert_eq!(t.retry(1), Duration::from_millis(200));
        let full = Timing::new(1.0);
        assert_eq!(full.connect(), Duration::from_secs(5));
        assert_eq!(full.keep_alive(), Duration::from_secs(20));
        assert_eq!(full.pairing(), Duration::from_secs(15));
        assert_eq!(full.end(), Duration::from_secs(10));
        assert_eq!(full.persist(), Duration::from_secs(5));
        assert_eq!(full.locked(), Duration::from_secs(600));
        assert_eq!(full.stale(), Duration::from_secs(20));
    }
}
