//! The coded-error contract (SA-ERR-01; ErrorHandling.md) and this crate's own error enum.

use serde::{Deserialize, Serialize};

/// Every code this crate owns (ErrorHandling.md *Internal Logic* 11): the `TypesError` and `YamlError`
/// codes, and the 2C `InputUsed` codes the registry places under Shared Types, which callers raise
/// through [`TypesError::Rule`].
pub const ALL_CODES: &[&str] = &[
    "HASH_NOT_SHA256_HEX",
    "ID_MEMORY_REF_INVALID",
    "ID_NOT_UUIDV7",
    "INPUT_LOCATION_INVALID",
    "INPUT_NAME_INVALID",
    "INPUT_VERSION_INVALID",
    "NUM_NOT_DECIMAL",
    "SRV_BAD_CURSOR",
    "TEXT_CONTROL_CHAR",
    "TS_NOT_RFC3339_MS_UTC",
    "YAML_FEATURE_REFUSED",
    "YAML_MALFORMED",
    "YAML_TOO_LARGE",
];

/// Every validation failure this crate raises. `code()` is the registry code.
///
/// `Display` never includes the refused value, so a caller logging the error cannot leak the input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TypesError {
    #[error("identifier is not a version-7 UUID")]
    IdNotUuidV7,
    #[error("timestamp is not RFC 3339 UTC with milliseconds")]
    TimestampNotRfc3339MsUtc,
    #[error("hash is not 64 lowercase hexadecimal characters")]
    HashNotSha256Hex,
    #[error("number is not a decimal string")]
    NumNotDecimal,
    #[error("text contains a control character")]
    TextControlChar,
    #[error("cursor is not one this server issued")]
    BadCursor,
    /// A rule whose code the caller supplied (lengths, names, paths, URLs, base64 lengths).
    #[error("value refused: {code}")]
    Rule { code: &'static str },
}

impl TypesError {
    /// The registry code: `ID_NOT_UUIDV7`, `TS_NOT_RFC3339_MS_UTC`, `HASH_NOT_SHA256_HEX`,
    /// `NUM_NOT_DECIMAL`, `TEXT_CONTROL_CHAR`, `SRV_BAD_CURSOR`, or the caller's `code`.
    pub fn code(&self) -> &'static str {
        match self {
            TypesError::IdNotUuidV7 => "ID_NOT_UUIDV7",
            TypesError::TimestampNotRfc3339MsUtc => "TS_NOT_RFC3339_MS_UTC",
            TypesError::HashNotSha256Hex => "HASH_NOT_SHA256_HEX",
            TypesError::NumNotDecimal => "NUM_NOT_DECIMAL",
            TypesError::TextControlChar => "TEXT_CONTROL_CHAR",
            TypesError::BadCursor => "SRV_BAD_CURSOR",
            TypesError::Rule { code } => code,
        }
    }
}

/// The contract every module error type implements, so that a program can turn any module
/// error into a Service API error reply without knowing the module (SA-ERR-01).
pub trait CodedError: std::error::Error + Send + Sync + 'static {
    /// The registry code, `<PREFIX>_<NAME>`.
    fn code(&self) -> &'static str;
    /// True when the same call may succeed if repeated unchanged later.
    fn retryable(&self) -> bool;
    /// The one repair action shown to the person, if any (07 — Error handling).
    fn repair(&self) -> Option<RepairAction> {
        None
    }
}

/// No error of this crate is retryable: each is a property of the value.
impl CodedError for TypesError {
    fn code(&self) -> &'static str {
        TypesError::code(self)
    }
    fn retryable(&self) -> bool {
        false
    }
}

/// The one action an error reply may offer the person (2C wire §1 *CommandReply*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RepairAction {
    TryAgain,
    SignInAgain,
    SendToIt,
}
