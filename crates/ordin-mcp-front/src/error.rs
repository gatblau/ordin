//! The MCP Front's error type and the codes it owns (McpFront.md *Error Table*).

use ordin_types::{CodedError, RepairAction};

use crate::names::ServerName;
use ordin_types::RunId;

/// Every code the registry gives this crate: its `MCP_` rows and the block-check codes that keep
/// their 2C prefix `BLOCK_` (McpFront.md *Approach*). Sorted.
pub const ALL_CODES: &[&str] = &[
    "BLOCK_PLAN_INVALID",
    "BLOCK_PLAN_POSITION_INVALID",
    "BLOCK_PLAN_TOO_MANY_VERSIONS",
    "BLOCK_REPORT_REFUSED",
    "MCP_ALREADY_STARTED",
    "MCP_ARGUMENTS_INVALID",
    "MCP_ARGUMENTS_TOO_LARGE",
    "MCP_AWAITING_APPROVAL",
    "MCP_BLOCK_STORE_ALREADY_BOUND",
    "MCP_BLOCK_STORE_NOT_BOUND",
    "MCP_BRIDGE_MALFORMED",
    "MCP_BRIDGE_PEER_REFUSED",
    "MCP_BRIDGE_PROTOCOL_UNSUPPORTED",
    "MCP_BRIDGE_UNAUTHENTICATED",
    "MCP_CALL_CANCELLED",
    "MCP_CALL_TIMEOUT",
    "MCP_DECISION_FAILED",
    "MCP_DENIED",
    "MCP_INVALID_NAME",
    "MCP_JOURNAL_FAILED",
    "MCP_NOT_STARTED",
    "MCP_OUTPUT_TRUNCATED",
    "MCP_PLAN_NOT_STATED",
    "MCP_PLAN_ONE_NOW",
    "MCP_PROVIDER_DUPLICATE",
    "MCP_PROVIDER_FAILED",
    "MCP_PROVIDER_PANICKED",
    "MCP_REF_RESOLVER_ALREADY_BOUND",
    "MCP_REF_RESOLVER_NOT_BOUND",
    "MCP_REGISTRY_FROZEN",
    "MCP_REJECTED",
    "MCP_REST_OFFSET_INVALID",
    "MCP_REST_UNKNOWN",
    "MCP_SERVER_CLAIMED",
    "MCP_SERVER_NOT_OFFERED",
    "MCP_SESSION_CLOSED",
    "MCP_SESSION_EXISTS",
    "MCP_SESSION_UNKNOWN",
    "MCP_SHUTTING_DOWN",
    "MCP_SOCKET_UNAVAILABLE",
    "MCP_TOOL_NOT_OFFERED",
    "MCP_TOOL_SPEC_INVALID",
    "MCP_TOO_MANY_CALLS",
];

/// In-process API errors. Tool-call failures are not errors of this type: they are
/// `ToolCallResult` values with `is_error` set.
#[derive(Debug, Clone, thiserror::Error)]
pub enum McpError {
    #[error("invalid name")]
    InvalidName { value: String },
    #[error("registry frozen")]
    RegistryFrozen,
    #[error("provider registered twice")]
    ProviderDuplicate { provider_id: &'static str },
    #[error("server already served by another provider")]
    ServerClaimed {
        server: ServerName,
        by: &'static str,
    },
    #[error("tool spec invalid")]
    ToolSpecInvalid {
        provider_id: &'static str,
        tool: String,
        rule: &'static str,
    },
    #[error("already started")]
    AlreadyStarted,
    #[error("not started")]
    NotStarted,
    #[error("socket unavailable")]
    SocketUnavailable { detail: String },
    #[error("ref resolver already bound")]
    RefResolverAlreadyBound,
    #[error("ref resolver not bound")]
    RefResolverNotBound,
    /// `bind_block_store` called a second time (McpFront.md amendment 2026-01-24).
    #[error("block store already bound")]
    BlockStoreAlreadyBound,
    #[error("session exists")]
    SessionExists { run_id: RunId },
    #[error("session unknown")]
    SessionUnknown { run_id: RunId },
    #[error("provider failed")]
    ProviderFailed {
        provider_id: &'static str,
        code: String,
        retryable: bool,
    },
}

impl McpError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            McpError::InvalidName { .. } => "MCP_INVALID_NAME",
            McpError::RegistryFrozen => "MCP_REGISTRY_FROZEN",
            McpError::ProviderDuplicate { .. } => "MCP_PROVIDER_DUPLICATE",
            McpError::ServerClaimed { .. } => "MCP_SERVER_CLAIMED",
            McpError::ToolSpecInvalid { .. } => "MCP_TOOL_SPEC_INVALID",
            McpError::AlreadyStarted => "MCP_ALREADY_STARTED",
            McpError::NotStarted => "MCP_NOT_STARTED",
            McpError::SocketUnavailable { .. } => "MCP_SOCKET_UNAVAILABLE",
            McpError::RefResolverAlreadyBound => "MCP_REF_RESOLVER_ALREADY_BOUND",
            McpError::RefResolverNotBound => "MCP_REF_RESOLVER_NOT_BOUND",
            McpError::BlockStoreAlreadyBound => "MCP_BLOCK_STORE_ALREADY_BOUND",
            McpError::SessionExists { .. } => "MCP_SESSION_EXISTS",
            McpError::SessionUnknown { .. } => "MCP_SESSION_UNKNOWN",
            McpError::ProviderFailed { .. } => "MCP_PROVIDER_FAILED",
        }
    }

    /// True for `MCP_SOCKET_UNAVAILABLE`, `MCP_NOT_STARTED` and `MCP_PROVIDER_FAILED` when the
    /// provider said so.
    pub fn retryable(&self) -> bool {
        match self {
            McpError::SocketUnavailable { .. } | McpError::NotStarted => true,
            McpError::ProviderFailed { retryable, .. } => *retryable,
            _ => false,
        }
    }
}

impl CodedError for McpError {
    fn code(&self) -> &'static str {
        McpError::code(self)
    }

    fn retryable(&self) -> bool {
        McpError::retryable(self)
    }

    fn repair(&self) -> Option<RepairAction> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::error_registry::{lookup, owns_prefix};

    /// X4: every code is registered to this crate, and its prefix is owned by it.
    #[test]
    fn x4_every_code_is_in_the_registry_and_its_prefix_is_owned() {
        assert!(
            ALL_CODES.windows(2).all(|w| w[0] < w[1]),
            "sorted and unique"
        );
        for code in ALL_CODES {
            let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
            assert_eq!(entry.owner_crate, "ordin-mcp-front", "{code}");
            let prefix = code.split('_').next().unwrap_or("");
            assert!(owns_prefix("ordin-mcp-front", prefix), "{code}");
        }
    }

    /// Every registry row the crate owns is listed, so no code is missing from `ALL_CODES`.
    #[test]
    fn x4_all_codes_lists_every_row_the_registry_gives_the_crate() {
        for prefix in ["MCP", "BLOCK"] {
            assert!(owns_prefix("ordin-mcp-front", prefix), "{prefix}");
        }
        assert_eq!(ALL_CODES.len(), 43);
    }

    #[test]
    fn every_error_has_a_listed_code_and_the_flags_of_the_error_table() {
        let run_id = ordin_types::RunId::new();
        let server = ServerName::new("memory").expect("name");
        let all = [
            (McpError::InvalidName { value: "X".into() }, false),
            (McpError::RegistryFrozen, false),
            (McpError::ProviderDuplicate { provider_id: "p" }, false),
            (McpError::ServerClaimed { server, by: "p" }, false),
            (
                McpError::ToolSpecInvalid {
                    provider_id: "p",
                    tool: "t".into(),
                    rule: "name",
                },
                false,
            ),
            (McpError::AlreadyStarted, false),
            (McpError::NotStarted, true),
            (McpError::SocketUnavailable { detail: "d".into() }, true),
            (McpError::RefResolverAlreadyBound, false),
            (McpError::RefResolverNotBound, false),
            (McpError::BlockStoreAlreadyBound, false),
            (McpError::SessionExists { run_id }, false),
            (McpError::SessionUnknown { run_id }, false),
            (
                McpError::ProviderFailed {
                    provider_id: "p",
                    code: "C".into(),
                    retryable: true,
                },
                true,
            ),
            (
                McpError::ProviderFailed {
                    provider_id: "p",
                    code: "C".into(),
                    retryable: false,
                },
                false,
            ),
        ];
        for (error, retryable) in all {
            assert!(ALL_CODES.contains(&error.code()), "{}", error.code());
            assert_eq!(error.retryable(), retryable, "{}", error.code());
            assert_eq!(CodedError::code(&error), error.code());
            assert!(CodedError::repair(&error).is_none());
            assert!(!error.to_string().is_empty());
        }
    }
}
