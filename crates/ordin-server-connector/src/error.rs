//! `ConnectorError`, the one error type of this crate, and the codes it owns (ServerConnector.md
//! *Public Interface*, *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate: the rows of the Error Table. `CONN_EVENT_UNREADABLE` is
/// a logged row with no error value; `SVC_EVENT_GAP` is logged here but owned by `ordin-service-api`,
/// so it is not listed.
pub const ALL_CODES: &[&str] = &[
    "CONN_ADDRESS_INVALID",
    "CONN_ADDRESS_LOCKED",
    "CONN_ALREADY_STARTED",
    "CONN_BAD_ANSWER",
    "CONN_CERT_UNTRUSTED",
    "CONN_CLIENT_CERT_UNAVAILABLE",
    "CONN_CODE_REFUSED",
    "CONN_COMPUTER_NAME_INVALID",
    "CONN_DATASTORE",
    "CONN_DEADLINE",
    "CONN_DUPLICATE",
    "CONN_EVENT_UNREADABLE",
    "CONN_FINGERPRINT_MISMATCH",
    "CONN_KEYCHAIN",
    "CONN_KEY_REFUSED",
    "CONN_NOT_CONNECTED",
    "CONN_PAIRING_LINE_INVALID",
    "CONN_PAIRING_STOPPED",
    "CONN_PORT_OUT_OF_RANGE",
    "CONN_REFUSED_FOR_VERSION",
    "CONN_TOO_MANY_DESKTOPS",
    "CONN_UNKNOWN_SERVER",
    "CONN_UNREACHABLE",
];

/// The code of the logged event-frame row (no error value carries it).
pub(crate) const CODE_EVENT_UNREADABLE: &str = "CONN_EVENT_UNREADABLE";
/// The code of the logged sequence-jump row, owned by `ordin-service-api`.
pub(crate) const CODE_EVENT_GAP: &str = "SVC_EVENT_GAP";

/// Every failure of this crate. `Display` writes a fixed phrase, never a key, a code, a body or a
/// server message.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConnectorError {
    #[error("server address invalid")]
    AddressInvalid,
    #[error("port out of range")]
    PortOutOfRange,
    #[error("already paired with this server")]
    Duplicate,
    #[error("pairing line invalid")]
    PairingLineInvalid,
    #[error("code refused")]
    CodeRefused,
    #[error("pairing stopped on the server")]
    PairingStopped,
    #[error("server has its most paired desktops")]
    TooManyDesktops,
    #[error("computer name refused")]
    ComputerNameInvalid,
    #[error("address locked by the server")]
    AddressLocked,
    #[error("server unreachable")]
    Unreachable,
    #[error("certificate not trusted")]
    CertificateUntrusted,
    #[error("certificate fingerprint changed")]
    FingerprintMismatch,
    #[error("client certificate unavailable")]
    ClientCertUnavailable,
    #[error("server version unusable")]
    RefusedForVersion { server_older: bool },
    #[error("not connected")]
    NotConnected,
    #[error("desktop key refused")]
    KeyRefused,
    #[error("server deadline")]
    Deadline,
    #[error("server answer unreadable")]
    BadAnswer,
    #[error("unknown server")]
    UnknownServer,
    #[error("keychain")]
    Keychain,
    #[error("datastore")]
    Datastore,
    #[error("already started")]
    AlreadyStarted,
}

impl CodedError for ConnectorError {
    fn code(&self) -> &'static str {
        match self {
            ConnectorError::AddressInvalid => "CONN_ADDRESS_INVALID",
            ConnectorError::PortOutOfRange => "CONN_PORT_OUT_OF_RANGE",
            ConnectorError::Duplicate => "CONN_DUPLICATE",
            ConnectorError::PairingLineInvalid => "CONN_PAIRING_LINE_INVALID",
            ConnectorError::CodeRefused => "CONN_CODE_REFUSED",
            ConnectorError::PairingStopped => "CONN_PAIRING_STOPPED",
            ConnectorError::TooManyDesktops => "CONN_TOO_MANY_DESKTOPS",
            ConnectorError::ComputerNameInvalid => "CONN_COMPUTER_NAME_INVALID",
            ConnectorError::AddressLocked => "CONN_ADDRESS_LOCKED",
            ConnectorError::Unreachable => "CONN_UNREACHABLE",
            ConnectorError::CertificateUntrusted => "CONN_CERT_UNTRUSTED",
            ConnectorError::FingerprintMismatch => "CONN_FINGERPRINT_MISMATCH",
            ConnectorError::ClientCertUnavailable => "CONN_CLIENT_CERT_UNAVAILABLE",
            ConnectorError::RefusedForVersion { .. } => "CONN_REFUSED_FOR_VERSION",
            ConnectorError::NotConnected => "CONN_NOT_CONNECTED",
            ConnectorError::KeyRefused => "CONN_KEY_REFUSED",
            ConnectorError::Deadline => "CONN_DEADLINE",
            ConnectorError::BadAnswer => "CONN_BAD_ANSWER",
            ConnectorError::UnknownServer => "CONN_UNKNOWN_SERVER",
            ConnectorError::Keychain => "CONN_KEYCHAIN",
            ConnectorError::Datastore => "CONN_DATASTORE",
            ConnectorError::AlreadyStarted => "CONN_ALREADY_STARTED",
        }
    }

    /// The rows the Error Table marks `Err(retryable)`.
    fn retryable(&self) -> bool {
        matches!(
            self,
            ConnectorError::AddressLocked
                | ConnectorError::Unreachable
                | ConnectorError::NotConnected
                | ConnectorError::Deadline
                | ConnectorError::BadAnswer
                | ConnectorError::Keychain
                | ConnectorError::Datastore
        )
    }

    /// The `try_again` action of the Error Table; no other row offers one.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            ConnectorError::Unreachable | ConnectorError::Deadline | ConnectorError::Keychain => {
                Some(RepairAction::TryAgain)
            }
            ConnectorError::FingerprintMismatch | ConnectorError::ClientCertUnavailable => None,
            _ => None,
        }
    }
}
