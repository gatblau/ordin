//! Name patterns of servers, tools and prefixes, checked without a regular-expression engine.

use serde::{Deserialize, Serialize};

use crate::error::McpError;

/// MCP server name: `^[a-z][a-z0-9_]{0,31}$`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerName(pub(crate) String);

impl ServerName {
    /// `Err(McpError::InvalidName)` when the value breaks the pattern.
    // spec: ordin-desktop/components/McpFront#LW-19
    pub fn new(value: impl Into<String>) -> Result<Self, McpError> {
        let value = value.into();
        if is_server_name(&value) {
            Ok(ServerName(value))
        } else {
            Err(McpError::InvalidName { value })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ServerName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `^[a-z][a-z0-9_]{0,31}$`
pub(crate) fn is_server_name(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && text.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `^[A-Za-z][A-Za-z0-9_-]{0,63}$` (SA-RECON-04).
// spec: ordin-desktop/components/McpFront#SA-RECON-04
pub(crate) fn is_tool_name(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && text.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `^[a-z][a-z0-9]{0,15}_$` (SA-RECON-06).
// spec: ordin-desktop/components/McpFront#SA-RECON-06
pub(crate) fn is_server_prefix(text: &str) -> bool {
    let Some(body) = text.strip_suffix('_') else {
        return false;
    };
    let mut chars = body.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && body.len() <= 16
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_names() {
        for good in [
            "a",
            "memory",
            "craft_artcraft",
            "x9_",
            "a1234567890123456789012345678901",
        ] {
            assert!(ServerName::new(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "Memory",
            "1a",
            "_a",
            "a-b",
            "a b",
            "a12345678901234567890123456789012",
        ] {
            let err = ServerName::new(bad).unwrap_err();
            assert_eq!(err.code(), "MCP_INVALID_NAME", "{bad}");
        }
        assert_eq!(ServerName::new("work").unwrap().as_str(), "work");
        assert_eq!(ServerName::new("work").unwrap().to_string(), "work");
    }

    #[test]
    fn tool_names() {
        assert!(is_tool_name("getTicket"));
        assert!(is_tool_name("note-write_2"));
        assert!(!is_tool_name("_x"));
        assert!(!is_tool_name(""));
        assert!(!is_tool_name(&"a".repeat(65)));
        assert!(is_tool_name(&"a".repeat(64)));
    }

    #[test]
    fn prefixes() {
        assert!(is_server_prefix("craft_"));
        assert!(!is_server_prefix("craft"));
        assert!(!is_server_prefix("_"));
        assert!(!is_server_prefix("Craft_"));
        assert!(!is_server_prefix("cr_aft_"));
        assert!(!is_server_prefix(&format!("{}_", "a".repeat(17))));
    }
}
