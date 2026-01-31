//! The 2C wire §1 types this crate defines, copied verbatim (ServiceApi.md *Shared Context*).

use ordin_types::{BackendId, CommandRequestId, RepairAction};
use serde::{Deserialize, Serialize};

use crate::events::ServiceEvent;
use ordin_types::time::Timestamp;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub struct ServiceApiVersion {
    pub major: u16,
    pub minor: u16,
}

impl ServiceApiVersion {
    pub const CURRENT: ServiceApiVersion = ServiceApiVersion { major: 1, minor: 0 };

    /// ADR-096 item 2: a desktop works with a server of the same major version and the same or the
    /// previous minor version.
    pub fn desktop_accepts(desktop: Self, server: Self) -> bool {
        desktop.major == server.major
            && (server.minor == desktop.minor
                || u32::from(server.minor) + 1 == u32::from(desktop.minor))
    }

    /// The *Validation rules* row: `major` 1–999, `minor` 0–999 (`SVC_VERSION_MALFORMED` otherwise).
    pub(crate) fn in_range(&self) -> bool {
        (1..=999).contains(&self.major) && self.minor <= 999
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CommandEnvelope<I> {
    pub api_version: ServiceApiVersion,
    /// Chosen by the caller; repeated on the reply.
    pub request_id: CommandRequestId,
    /// `<module>.<verb>` in snake_case, for example `approval.decide`.
    pub command: CommandName,
    /// Which backend the command is for.
    pub backend: BackendId,
    pub input: I,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandName(String);

impl CommandName {
    /// Wraps a dotted command name; the envelope check applies the name pattern.
    pub fn new(name: impl Into<String>) -> Self {
        CommandName(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CommandReply<O> {
    pub request_id: CommandRequestId,
    #[serde(flatten)]
    pub outcome: CommandOutcome<O>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandOutcome<O> {
    Ok { result: O },
    Error { error: ServiceError },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ServiceError {
    /// Stable code, never shown to the person (07 — Error handling).
    pub code: String,
    /// One plain-language sentence in the person's locale, built by Localisation.
    pub message: String,
    /// At most one repair action (07 — Error handling).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<RepairAction>,
    /// The Diagnostics line identifier, shown as "Reference r-81" under an error that offers a repair
    /// (11 §6, UX-A70); never a path, token or internal state (2C wire §1, decision 80: C-30).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EventEnvelope {
    pub api_version: ServiceApiVersion,
    pub backend: BackendId,
    /// The backend's journal sequence number: strictly increasing per backend, starting at 1, never
    /// reused. On SSE it is also the `id:` line.
    pub seq: u64,
    pub at: Timestamp,
    #[serde(flatten)]
    pub event: ServiceEvent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_pairs() {
        let v = |major, minor| ServiceApiVersion { major, minor };
        assert!(ServiceApiVersion::desktop_accepts(v(1, 0), v(1, 0)));
        assert!(ServiceApiVersion::desktop_accepts(v(1, 1), v(1, 0)));
        assert!(!ServiceApiVersion::desktop_accepts(v(1, 2), v(1, 0)));
        assert!(!ServiceApiVersion::desktop_accepts(v(1, 0), v(1, 1)));
        assert!(!ServiceApiVersion::desktop_accepts(v(2, 0), v(1, 0)));
        assert!(!ServiceApiVersion::desktop_accepts(v(1, 0), v(1, u16::MAX)));
        assert!(v(1, 0).in_range());
        assert!(!v(0, 0).in_range());
        assert!(!v(1000, 0).in_range());
        assert!(!v(1, 1000).in_range());
        assert_eq!(CommandName::new("a.b").as_str(), "a.b");
    }
}
