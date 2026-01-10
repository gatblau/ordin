//! The module's error enum and its codes (OrganisationProfile.md *Error Table*; ErrorHandling.md
//! *Internal Logic* 1 and 11).

use ordin_types::{CodedError, RepairAction};

/// Every code this crate owns: the 21 `PROF_` codes and the three `RAG_` codes of the 2C
/// `RemoteAgent` rules (ErrorHandling.md *Error-code prefixes and owners*).
pub const ALL_CODES: &[&str] = &[
    "PROF_ADDRESS_INVALID",
    "PROF_CACHE_INVALID",
    "PROF_CONTENT_INVALID",
    "PROF_DATASTORE_UNAVAILABLE",
    "PROF_FORMAT_UNSUPPORTED",
    "PROF_HTTP_STATUS",
    "PROF_INSTALLER_INVALID",
    "PROF_INSTALLER_WRITE_FAILED",
    "PROF_KEY_UNKNOWN",
    "PROF_MULTIPLE_ACTIVE",
    "PROF_NOT_USED",
    "PROF_NO_ADDRESS",
    "PROF_REMOTE_AGENT_NOT_ENABLED",
    "PROF_REMOTE_AGENT_NOT_FOUND",
    "PROF_REMOTE_AGENT_WITHDRAWN",
    "PROF_SIGNATURE_INVALID",
    "PROF_SIGNATURE_MISSING",
    "PROF_TOO_LARGE",
    "PROF_UNREACHABLE",
    "PROF_UNVERIFIED_ACTIVATION",
    "PROF_VERSION_NOT_NEWER",
    "RAG_HTTP_FIELDS_INVALID",
    "RAG_NAME_INVALID",
    "RAG_SLACK_FIELDS_INVALID",
];

/// Every failure of the Organisation Profile, one variant per *Error Table* row.
///
/// `Display` never holds a fetched value, an address or key material (Logging.md R-7); the dotted key
/// path of a content failure is the only detail carried, and it names a document key, never a value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("no profile address is known")]
    NoAddress,
    #[error("the profile address cannot be reached")]
    Unreachable,
    #[error("the profile address answered HTTP {status}")]
    HttpStatus { status: u16 },
    #[error("the profile document or its signature is too large")]
    TooLarge,
    #[error("the profile signature file is absent")]
    SignatureMissing,
    #[error("the profile signature is malformed or does not verify")]
    SignatureInvalid,
    #[error("the profile is signed with a key the installer does not carry")]
    KeyUnknown,
    #[error("the profile format version is not read by this build")]
    FormatUnsupported,
    /// `key_path` is the dotted path of the first refused key (for example `usage.agents_at_once_max`).
    #[error("the profile content is invalid at {key_path}")]
    ContentInvalid { key_path: String },
    #[error("the profile version is not newer than the active one")]
    VersionNotNewer,
    #[error("a profile that is not verified was about to become active")]
    UnverifiedActivation,
    #[error("a second active profile row was refused")]
    MultipleActive,
    #[error("the datastore cannot be read or written")]
    DatastoreUnavailable,
    #[error("the installer settings are not valid")]
    InstallerInvalid,
    #[error("the profile address is not an https address of at most 2048 bytes")]
    AddressInvalid,
    #[error("the installer settings cannot be written")]
    InstallerWriteFailed,
    #[error("the cached profile fails verification")]
    CacheInvalid,
    #[error("this program does not use an organisation profile")]
    NotUsed,
    #[error("the remote agent is unknown")]
    RemoteAgentNotFound,
    #[error("the remote agent is withdrawn")]
    RemoteAgentWithdrawn,
    #[error("the remote agent is not enabled")]
    RemoteAgentNotEnabled,
    #[error("a remote agent's name breaks the 2C rule")]
    RemoteAgentNameInvalid,
    #[error("a remote agent's http fields break the 2C rule")]
    RemoteAgentHttpFieldsInvalid,
    #[error("a remote agent's Slack fields break the 2C rule")]
    RemoteAgentSlackFieldsInvalid,
}

impl ProfileError {
    /// The content refusal at `key_path`.
    pub(crate) fn content(key_path: impl Into<String>) -> Self {
        ProfileError::ContentInvalid {
            key_path: key_path.into(),
        }
    }

    /// True for the failures that end a refresh with `RefreshOutcome::Refused` rather than an error:
    /// the address answered, and what it served is not used (Error Table rows marked "→ Refused").
    pub(crate) fn is_refusal(&self) -> bool {
        matches!(
            self,
            ProfileError::TooLarge
                | ProfileError::SignatureMissing
                | ProfileError::SignatureInvalid
                | ProfileError::KeyUnknown
                | ProfileError::FormatUnsupported
                | ProfileError::ContentInvalid { .. }
                | ProfileError::VersionNotNewer
        )
    }

    /// The text stored in `organisation_profiles.refusal_reason`: the code and, for a content failure,
    /// the dotted key path, at most 500 characters (Internal Logic 2.8).
    pub(crate) fn refusal_reason(&self) -> String {
        let reason = match self {
            ProfileError::ContentInvalid { key_path } => format!("{} {key_path}", self.code()),
            other => other.code().to_owned(),
        };
        reason.chars().take(500).collect()
    }
}

impl CodedError for ProfileError {
    fn code(&self) -> &'static str {
        match self {
            ProfileError::NoAddress => "PROF_NO_ADDRESS",
            ProfileError::Unreachable => "PROF_UNREACHABLE",
            ProfileError::HttpStatus { .. } => "PROF_HTTP_STATUS",
            ProfileError::TooLarge => "PROF_TOO_LARGE",
            ProfileError::SignatureMissing => "PROF_SIGNATURE_MISSING",
            ProfileError::SignatureInvalid => "PROF_SIGNATURE_INVALID",
            ProfileError::KeyUnknown => "PROF_KEY_UNKNOWN",
            ProfileError::FormatUnsupported => "PROF_FORMAT_UNSUPPORTED",
            ProfileError::ContentInvalid { .. } => "PROF_CONTENT_INVALID",
            ProfileError::VersionNotNewer => "PROF_VERSION_NOT_NEWER",
            ProfileError::UnverifiedActivation => "PROF_UNVERIFIED_ACTIVATION",
            ProfileError::MultipleActive => "PROF_MULTIPLE_ACTIVE",
            ProfileError::DatastoreUnavailable => "PROF_DATASTORE_UNAVAILABLE",
            ProfileError::InstallerInvalid => "PROF_INSTALLER_INVALID",
            ProfileError::AddressInvalid => "PROF_ADDRESS_INVALID",
            ProfileError::InstallerWriteFailed => "PROF_INSTALLER_WRITE_FAILED",
            ProfileError::CacheInvalid => "PROF_CACHE_INVALID",
            ProfileError::NotUsed => "PROF_NOT_USED",
            ProfileError::RemoteAgentNotFound => "PROF_REMOTE_AGENT_NOT_FOUND",
            ProfileError::RemoteAgentWithdrawn => "PROF_REMOTE_AGENT_WITHDRAWN",
            ProfileError::RemoteAgentNotEnabled => "PROF_REMOTE_AGENT_NOT_ENABLED",
            ProfileError::RemoteAgentNameInvalid => "RAG_NAME_INVALID",
            ProfileError::RemoteAgentHttpFieldsInvalid => "RAG_HTTP_FIELDS_INVALID",
            ProfileError::RemoteAgentSlackFieldsInvalid => "RAG_SLACK_FIELDS_INVALID",
        }
    }

    /// Error Table *Status* column: retryable for an unreachable address, HTTP 408, 429 and 5xx, a
    /// datastore failure and an installer file that cannot be written; permanent otherwise.
    fn retryable(&self) -> bool {
        match self {
            ProfileError::Unreachable
            | ProfileError::DatastoreUnavailable
            | ProfileError::InstallerWriteFailed => true,
            ProfileError::HttpStatus { status } => {
                *status == 408 || *status == 429 || (500..=599).contains(status)
            }
            _ => false,
        }
    }

    /// Error Table *Response Body* `action`: `send_to_it` with no address, `try_again` for an
    /// unreachable address and a datastore failure; for an HTTP status, `try_again` when retryable
    /// and `send_to_it` otherwise.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            ProfileError::NoAddress => Some(RepairAction::SendToIt),
            ProfileError::Unreachable | ProfileError::DatastoreUnavailable => {
                Some(RepairAction::TryAgain)
            }
            ProfileError::HttpStatus { .. } => Some(if self.retryable() {
                RepairAction::TryAgain
            } else {
                RepairAction::SendToIt
            }),
            _ => None,
        }
    }
}
